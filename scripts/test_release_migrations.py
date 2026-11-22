#!/usr/bin/env python3
"""Tests of scripts/release-migrations.py and release_simplify.py (run by `cargo test`,
crates/musicus-server/tests/updates.rs::release_script_unit_tests, or directly)."""

from __future__ import annotations

import importlib.util
import sqlite3
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import release_simplify as rs  # noqa: E402

spec = importlib.util.spec_from_file_location("release_migrations", HERE / "release-migrations.py")
rm = importlib.util.module_from_spec(spec)
spec.loader.exec_module(rm)


def run(sql: str) -> sqlite3.Connection:
    conn = sqlite3.connect(":memory:")
    conn.executescript(sql)
    return conn


class Rules(unittest.TestCase):
    def check(self, sql: str, expect: list[str]) -> str:
        out, notes = rs.simplify(sql)
        self.assertEqual([n.split(":")[0] for n in notes], expect, notes)
        if notes:
            self.assertIsNone(rs.verify([], sql, out, None), out)
        return out

    def test_fold_add_before_constraints_and_comments(self):
        out = self.check(
            "CREATE TABLE t (\n  id INTEGER PRIMARY KEY,\n  a TEXT -- note, (with) commas\n  , UNIQUE (a)\n);\n"
            "CREATE INDEX t_a ON t(a);\n"
            "CREATE TABLE u (t_id INTEGER REFERENCES t(id));\n"
            "ALTER TABLE t ADD COLUMN b INTEGER NOT NULL DEFAULT 0;\n",
            ["fold-add"],
        )
        self.assertNotIn("ALTER", out)
        self.assertEqual([r[1] for r in run(out).execute("PRAGMA table_info(t)")], ["id", "a", "b"])

    def test_fold_rename(self):
        out = self.check("CREATE TABLE t (id INTEGER, message TEXT);\nALTER TABLE t RENAME COLUMN message TO problem;\n",
                         ["fold-rename"])
        self.assertIn("problem TEXT", out)

    def test_drop_pairs(self):
        out = self.check(
            "CREATE TABLE keep (x);\nCREATE TABLE tmp (x);\nCREATE INDEX tmp_x ON tmp(x);\n"
            "CREATE TRIGGER tr AFTER INSERT ON keep BEGIN SELECT 1; END;\n"
            "DROP TRIGGER tr;\nDROP TABLE tmp;\n"
            "ALTER TABLE keep ADD COLUMN y;\nALTER TABLE keep DROP COLUMN y;\n",
            ["drop-pair", "drop-pair", "column-pair"],
        )
        self.assertEqual([r[0] for r in run(out).execute("SELECT name FROM sqlite_master")], ["keep"])

    def test_no_rule_across_a_data_step(self):
        # An UPDATE reads the column between ADD and DROP; an INSERT writes the table before the ADD.
        self.check("CREATE TABLE t (a);\nINSERT INTO t VALUES (1);\nALTER TABLE t ADD COLUMN b;\n"
                   "UPDATE t SET a = b;\nALTER TABLE t DROP COLUMN b;\n", [])
        # A trigger on the table and a view also block.
        self.check("CREATE TABLE t (a);\nCREATE VIEW v AS SELECT * FROM t;\nALTER TABLE t ADD COLUMN b;\n", [])
        # A table created by an earlier release is not folded.
        self.check("ALTER TABLE old ADD COLUMN b;\n", [])
        # Rename when an index between uses the old name.
        self.check("CREATE TABLE t (a, b);\nCREATE INDEX t_a ON t(a);\nALTER TABLE t RENAME COLUMN a TO c;\n", [])

    def test_statements_split_with_triggers_and_strings(self):
        chunks = rs.split("-- c\nCREATE TRIGGER x AFTER INSERT ON t BEGIN\n  INSERT INTO u VALUES (';');\nEND;\n")
        self.assertEqual([c.stmt for c in chunks], [False, True])
        self.assertEqual(chunks[1].info, {"op": "create", "kind": "trigger", "name": "x", "on": "t"})


class Verification(unittest.TestCase):
    def test_a_wrong_simplification_is_rejected(self):
        appended = "CREATE TABLE t (a);\nALTER TABLE t ADD COLUMN b DEFAULT 1;\n"
        self.assertIn("schema differs at table `t`", rs.verify([], appended, "CREATE TABLE t (a, b DEFAULT 2);\n", None))
        self.assertIn("fails", rs.verify([], appended, "CREATE TABLE t (a, b;\n", None))

    def test_fixture_rows_are_compared(self):
        with tempfile.TemporaryDirectory() as tmp:
            fixture = Path(tmp) / "prev.sqlite"
            conn = sqlite3.connect(fixture)
            conn.executescript("CREATE TABLE t (a); INSERT INTO t VALUES (1), (2);")
            conn.close()
            appended = "CREATE TABLE n (x);\nUPDATE t SET a = a + 1;\n"
            wrong = "CREATE TABLE n (x);\n"
            self.assertIsNone(rs.verify(["CREATE TABLE t (a);"], appended, wrong, None), "no rows without the fixture")
            self.assertEqual(rs.verify(["CREATE TABLE t (a);"], appended, wrong, fixture), "rows differ in table `t` (on the fixture)")


class Merge(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = Path(self.tmp.name)
        (self.dir / "preview").mkdir()
        (self.dir / "releases.txt").write_text("1.0\n")
        (self.dir / "1.0.0.sql").write_text("-- @additive\nCREATE TABLE t (a);\n")

    def tearDown(self):
        self.tmp.cleanup()

    def preview(self, name: str, text: str):
        (self.dir / "preview" / name).write_text(text)

    def test_merge_simplifies_and_verifies(self):
        self.preview("0001-n.sql", "-- @additive\nCREATE TABLE n (x);\n")
        self.preview("0002-n-y.sql", "-- @additive\nALTER TABLE n ADD COLUMN y TEXT;\n-- @removal\nALTER TABLE t DROP COLUMN a;\n")
        done = rm.merge(self.dir, "1.1.0")
        self.assertIn("simplified (fold-add: column `y` into CREATE TABLE `n`)", done)
        text = (self.dir / "1.1.0.sql").read_text()
        self.assertIn("-- @previews: 0001-n 0002-n-y\n", text)
        self.assertNotIn("ADD COLUMN", text)
        self.assertIn("-- @removal\n", text)
        self.assertFalse((self.dir / "preview").exists())

    def test_merge_writes_the_appended_version_when_verification_fails(self):
        self.preview("0001-n.sql", "-- @additive\nCREATE TABLE n (x);\nALTER TABLE n ADD COLUMN y;\n")
        original = rs.simplify
        rs.simplify = lambda sql: ("CREATE TABLE n (x, z);\n", ["fold-add: wrong"])
        try:
            done = rm.merge(self.dir, "1.1.0")
        finally:
            rs.simplify = original
        self.assertTrue(any(d.startswith("simplification discarded") and "`n`" in d for d in done), done)
        self.assertIn("ALTER TABLE n ADD COLUMN y;", (self.dir / "1.1.0.sql").read_text())

    def test_compare_files(self):
        a = self.dir / "a" / "1.0.0.sql"
        b = self.dir / "b" / "1.0.0.sql"
        a.parent.mkdir(), b.parent.mkdir()
        a.write_text("-- @previews: 0001-x\n-- @additive\nCREATE TABLE t (a);\nALTER TABLE t ADD COLUMN b;\n")
        b.write_text("-- by hand\n-- @previews: 0001-x\n-- @additive\nCREATE TABLE t (\n  a,\n  b\n);\n")
        self.assertIsNone(rm.compare_files(a, b, None))
        b.write_text("-- @previews: 0001-x\n-- @additive\nCREATE TABLE t (a);\n")
        self.assertIn("schema differs", rm.compare_files(a, b, None))
        b.write_text("-- @additive\nCREATE TABLE t (a, b);\n")
        self.assertIn("previews", rm.compare_files(a, b, None))

    def test_no_simplify(self):
        self.preview("0001-n.sql", "-- @additive\nCREATE TABLE n (x);\nALTER TABLE n ADD COLUMN y;\n")
        rm.merge(self.dir, "1.1.0", simplify=False)
        self.assertIn("ADD COLUMN y", (self.dir / "1.1.0.sql").read_text())


if __name__ == "__main__":
    unittest.main()
