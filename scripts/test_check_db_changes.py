#!/usr/bin/env python3
"""Tests of scripts/check-db-changes.py on temporary git repositories (run by `cargo test`,
crates/musicus-server/tests/updates.rs::db_check_script_unit_tests, or directly)."""

from __future__ import annotations

import importlib.util
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
SCRIPT = HERE / "check-db-changes.py"
spec = importlib.util.spec_from_file_location("check_db_changes", SCRIPT)
cdc = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cdc)

M = "crates/musicus-server/migrations/"
P = M + "preview/"
SCHEMA = "crates/musicus-server/schema.sql"


class Repo:
    """A repository with `main` (two previews, releases.txt, schema.sql) and a branch `pr`."""

    def __init__(self, root: Path):
        self.root = root
        self.git("init", "-q", "-b", "main")
        self.write({P + "0001-init.sql": "-- @additive\n", P + "0002-b.sql": "-- @additive\n",
                    M + "releases.txt": "", SCHEMA: "-- schema\n", "README.md": "x\n"})
        self.commit("base")
        self.git("switch", "-q", "-c", "pr")

    def git(self, *args: str) -> str:
        env = {**os.environ, "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@t", "GIT_COMMITTER_NAME": "t",
               "GIT_COMMITTER_EMAIL": "t@t", "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1"}
        return subprocess.run(["git", *args], cwd=self.root, check=True, capture_output=True, text=True,
                              env=env).stdout

    def write(self, files: dict[str, str]) -> None:
        for name, text in files.items():
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text)

    def commit(self, msg: str) -> None:
        self.git("add", "-A")
        self.git("commit", "-q", "--allow-empty", "-m", msg)

    def run(self, *args: str) -> subprocess.CompletedProcess:
        env = {k: v for k, v in os.environ.items() if k != "GITHUB_ACTIONS"}
        return subprocess.run([sys.executable, str(SCRIPT), *args], cwd=self.root, capture_output=True, text=True,
                              env=env)


class CheckDbChanges(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.repo = Repo(Path(self.tmp.name))

    def tearDown(self):
        self.tmp.cleanup()

    def check(self, branch: str = "feature/x") -> tuple[int, str]:
        r = self.repo.run("--branch", branch, "main")
        return r.returncode, r.stdout + r.stderr

    def test_new_preview_with_schema_passes(self):
        self.repo.write({P + "0003-topic.sql": "-- @additive\nCREATE TABLE t (id INTEGER);\n", SCHEMA: "-- new\n"})
        self.repo.commit("preview")
        code, out = self.check()
        self.assertEqual(code, 0, out)
        self.assertIn("1 new preview update(s): " + P + "0003-topic.sql", out)
        listed = self.repo.run("--list-new-previews", "main").stdout.split()
        self.assertEqual(listed, [P + "0003-topic.sql"])

    def test_unrelated_change_passes_and_lists_nothing(self):
        self.repo.write({"README.md": "y\n"})
        self.repo.commit("docs")
        self.assertEqual(self.check()[0], 0)
        self.assertEqual(self.repo.run("--list-new-previews", "main").stdout, "")

    def test_a_preview_without_schema_change_passes(self):
        # A preview may change only data (e.g. empty a search index).
        self.repo.write({P + "0003-data.sql": "-- @additive\nDELETE FROM x;\n"})
        self.repo.commit("data")
        self.assertEqual(self.check()[0], 0)

    def test_editing_a_merged_preview_fails(self):
        self.repo.write({P + "0002-b.sql": "-- @additive\nCREATE TABLE u (id INTEGER);\n"})
        self.repo.commit("edit")
        code, out = self.check()
        self.assertEqual(code, 1)
        self.assertIn(P + "0002-b.sql: modified", out)
        self.assertIn("add a new preview", out)

    def test_deleting_or_renaming_a_merged_preview_fails(self):
        self.repo.git("mv", P + "0002-b.sql", P + "0003-b.sql")
        self.repo.commit("rename")
        code, out = self.check()
        self.assertEqual(code, 1)
        self.assertIn(P + "0002-b.sql: deleted (or renamed)", out)

    def test_a_release_update_outside_a_release_branch_fails(self):
        self.repo.write({M + "1.0.0.sql": "-- @additive\n"})
        self.repo.commit("release update")
        code, out = self.check()
        self.assertEqual(code, 1)
        self.assertIn(M + "1.0.0.sql: only a release pull request", out)

    def test_releases_txt_changes_only_in_release_branches(self):
        self.repo.write({M + "releases.txt": "1.0\n"})
        self.repo.commit("line")
        code, out = self.check()
        self.assertEqual(code, 1)
        self.assertIn("releases.txt changes only in a release pull request", out)

    def test_badly_named_preview_fails(self):
        self.repo.write({P + "3-Topic.sql": "-- @additive\n", P + "README": "x\n"})
        self.repo.commit("names")
        code, out = self.check()
        self.assertEqual(code, 1)
        self.assertEqual(out.count("is named"), 2, out)

    def test_schema_change_without_preview_fails(self):
        self.repo.write({SCHEMA: "-- edited by hand\n"})
        self.repo.commit("schema")
        code, out = self.check()
        self.assertEqual(code, 1)
        self.assertIn("schema.sql changed, but the pull request adds no preview update", out)

    def test_schema_layout_change_with_its_generator_passes(self):
        self.repo.write({SCHEMA: "-- new layout\n", "crates/musicus-server/src/updates/schema.rs": "// layout\n"})
        self.repo.commit("layout")
        self.assertEqual(self.check()[0], 0)

    def test_introducing_the_schema_snapshot_passes(self):
        self.repo.git("switch", "-q", "main")
        self.repo.git("rm", "-q", SCHEMA)
        self.repo.commit("no snapshot yet")
        self.repo.git("switch", "-q", "-C", "pr")
        self.repo.write({SCHEMA: "-- schema\n"})
        self.repo.commit("snapshot")
        self.assertEqual(self.check()[0], 0)
        self.repo.git("rm", "-q", SCHEMA)
        self.repo.commit("deleted again")
        self.repo.git("switch", "-q", "main")
        self.repo.git("merge", "-q", "--ff-only", "pr~1")
        self.repo.git("switch", "-q", "pr")
        code, out = self.check()
        self.assertEqual(code, 1, out)

    def test_number_already_taken_on_main_fails(self):
        self.repo.write({P + "0003-mine.sql": "-- @additive\n"})
        self.repo.commit("mine")
        self.repo.git("switch", "-q", "main")
        self.repo.write({P + "0003-other.sql": "-- @additive\n"})
        self.repo.commit("other branch merged first")
        self.repo.git("switch", "-q", "pr")
        code, out = self.check()
        self.assertEqual(code, 1)
        self.assertIn("renumber this preview to 0004", out)

    def test_new_previews_need_distinct_numbers(self):
        self.repo.write({P + "0003-a.sql": "-- @additive\n", P + "0003-b.sql": "-- @additive\n"})
        self.repo.commit("two")
        code, out = self.check()
        self.assertEqual(code, 1)
        self.assertIn("several new previews have the number 0003", out)

    def test_main_moving_on_does_not_count_as_a_change(self):
        self.repo.git("switch", "-q", "main")
        self.repo.write({P + "0003-other.sql": "-- @additive\n", SCHEMA: "-- other\n"})
        self.repo.commit("other branch merged")
        self.repo.git("switch", "-q", "pr")
        self.repo.write({"README.md": "y\n"})
        self.repo.commit("docs")
        self.assertEqual(self.check()[0], 0)

    def test_release_branch_is_exempt(self):
        self.repo.git("rm", "-q", P + "0001-init.sql", P + "0002-b.sql")
        self.repo.write({M + "1.0.0.sql": "-- @previews: 0001-init 0002-b\n-- @additive\n", M + "releases.txt": "1.0\n"})
        self.repo.commit("release")
        code, out = self.check("release/1.0.0")
        self.assertEqual(code, 0, out)
        self.assertIn("release check (R-OPS-040)", out)
        # Only release/X.Y.Z counts.
        self.assertEqual(self.check("release/next")[0], 1)
        self.assertEqual(self.check("feature/release/1.0.0")[0], 1)

    def test_github_annotations(self):
        self.repo.write({P + "0002-b.sql": "changed\n"})
        self.repo.commit("edit")
        env = {**os.environ, "GITHUB_ACTIONS": "true"}
        r = subprocess.run([sys.executable, str(SCRIPT), "main"], cwd=self.repo.root, capture_output=True, text=True,
                           env=env)
        self.assertEqual(r.returncode, 1)
        self.assertIn(f"::error file={P}0002-b.sql::modified", r.stdout)


if __name__ == "__main__":
    unittest.main()
