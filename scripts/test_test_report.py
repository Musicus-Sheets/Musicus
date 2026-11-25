#!/usr/bin/env python3
"""Tests of scripts/test-report.py (a-test-report). Standard library only; run directly or through
`cargo test` (crates/musicus-server/tests/ci_tools.rs)."""

from __future__ import annotations

import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("test_report", HERE / "test-report.py")
tr = importlib.util.module_from_spec(spec)
spec.loader.exec_module(tr)

JUNIT = """<?xml version="1.0" encoding="UTF-8"?>
<testsuites name="nextest-run" tests="5">
  <testsuite name="musicus-api" tests="3">
    <testcase name="ink::tests::round_trip_keeps_every_object" classname="musicus-api" time="0.01"/>
    <testcase name="layout::tests::bands_merge" classname="musicus-api" time="0.02"/>
    <testcase name="naming::tests::keys" classname="musicus-api" time="0.03">
      <failure message="assertion failed">left != right</failure>
    </testcase>
  </testsuite>
  <testsuite name="musicus-server::api" tests="2">
    <testcase name="user_administration" classname="musicus-server::api" time="1.5">
      <flakyFailure message="timeout">first try</flakyFailure>
    </testcase>
    <testcase name="ignored_one" classname="musicus-server::api" time="0"><skipped/></testcase>
  </testsuite>
  <testsuite name="musicus-web::bin/musicus-web" tests="1">
    <testcase name="names::tests::part_name" classname="musicus-web::bin/musicus-web" time="0.01"/>
  </testsuite>
</testsuites>
"""


def test(suite: str, name: str, status: str = "passed") -> dict:
    return {"suite": suite, "name": name, "status": status, "time": 0.0}


class Inputs(unittest.TestCase):
    def setUp(self) -> None:
        self.dir = Path(tempfile.mkdtemp())

    def test_junit_states(self) -> None:
        path = self.dir / "junit.xml"
        path.write_text(JUNIT, encoding="utf-8")
        tests = tr.read_junit([path])
        self.assertEqual(len(tests), 6)
        self.assertEqual(tests["musicus-api::ink::tests::round_trip_keeps_every_object"]["status"], "passed")
        self.assertEqual(tests["musicus-api::naming::tests::keys"]["status"], "failed")
        self.assertEqual(tests["musicus-server::api::user_administration"]["status"], "flaky")
        self.assertEqual(tests["musicus-server::api::ignored_one"]["status"], "skipped")
        self.assertEqual(tr.counts(tests), {"failed": 1, "flaky": 1, "passed": 3, "skipped": 1})

    def test_several_junit_files_add_up(self) -> None:
        a, b = self.dir / "a.xml", self.dir / "b.xml"
        a.write_text(JUNIT, encoding="utf-8")
        b.write_text('<testsuite name="e2e/smoke.mjs"><testcase name="login" time="2"/></testsuite>', encoding="utf-8")
        tests = tr.read_junit([a, b])
        self.assertIn("e2e/smoke.mjs::login", tests)
        self.assertEqual(len(tests), 7)

    def test_lcov_relative_paths_and_dependencies_dropped(self) -> None:
        root = self.dir / "repo"
        (root / "crates/musicus-api/src").mkdir(parents=True)
        lcov = self.dir / "lcov.info"
        lcov.write_text(
            f"SF:{root}/crates/musicus-api/src/ink.rs\nDA:1,3\nDA:2,0\nDA:3,1\nend_of_record\n"
            "SF:/home/x/.cargo/registry/src/serde/lib.rs\nDA:1,1\nend_of_record\n"
            f"SF:{root}/crates/musicus-server/src/db.rs\nDA:5,0\nend_of_record\n",
            encoding="utf-8",
        )
        files = tr.read_lcov(lcov, root)
        self.assertEqual(files, {
            "crates/musicus-api/src/ink.rs": {"lines": 3, "hit": 2},
            "crates/musicus-server/src/db.rs": {"lines": 1, "hit": 0},
        })
        summary = tr.coverage_summary(files)
        self.assertEqual((summary["lines"], summary["hit"]), (4, 2))
        self.assertEqual(summary["groups"]["musicus-api"], {"lines": 3, "hit": 2})
        self.assertEqual(tr.percent(2, 4), 50.0)
        self.assertIsNone(tr.percent(0, 0))


class BrowserCoverage(unittest.TestCase):
    def setUp(self) -> None:
        self.root = Path(tempfile.mkdtemp())
        (self.root / "crates/musicus-server/src").mkdir(parents=True)

    def lcov(self, name: str, records: dict[str, str]) -> Path:
        path = self.root / name
        path.write_text("".join(f"SF:{self.root}/{f}\n{da}end_of_record\n" for f, da in records.items()),
                        encoding="utf-8")
        return path

    def test_unit_lines_are_the_reference(self) -> None:
        unit = tr.read_lcov_lines(self.lcov("u.info", {
            "crates/musicus-server/src/db.rs": "DA:1,1\nDA:2,0\nDA:3,0\nDA:4,0\n"}), self.root)
        # The release build lists line 9 that the unit build does not: it is left out.
        e2e = tr.read_lcov_lines(self.lcov("e.info", {
            "crates/musicus-server/src/db.rs": "DA:2,5\nDA:3,0\nDA:9,1\n",
            "crates/musicus-server/src/main.rs": "DA:1,1\nDA:2,0\n"}), self.root)
        parts = tr.combine_lcov(unit, e2e)
        db = "crates/musicus-server/src/db.rs"
        self.assertEqual(parts["unit"][db], {"lines": 4, "hit": 1})
        self.assertEqual(parts["e2e"][db], {"lines": 4, "hit": 1})
        self.assertEqual(parts["combined"][db], {"lines": 4, "hit": 2})
        # A file only the browser run has: its own lines are the reference.
        self.assertEqual(parts["combined"]["crates/musicus-server/src/main.rs"], {"lines": 2, "hit": 1})
        self.assertEqual(parts["unit"]["crates/musicus-server/src/main.rs"], {"lines": 2, "hit": 0})

    def test_report_shows_unit_browser_and_together(self) -> None:
        junit = self.root / "junit.xml"
        junit.write_text(JUNIT, encoding="utf-8")
        u = self.lcov("u.info", {"crates/musicus-server/src/db.rs": "DA:1,1\nDA:2,0\nDA:3,0\nDA:4,0\n"})
        e1 = self.lcov("e1.info", {"crates/musicus-server/src/db.rs": "DA:2,1\n"})
        e2 = self.lcov("e2.info", {"crates/musicus-server/src/db.rs": "DA:3,1\n"})
        out = self.root / "r.md"
        tr.main(["--junit", str(junit), "--lcov", str(u), "--lcov-e2e", str(e1), "--lcov-e2e", str(e2),
                 "--root", str(self.root), "--json", str(self.root / "r.json"), "--markdown", str(out)])
        md = out.read_text(encoding="utf-8")
        self.assertIn("### Code coverage: 75.0 % of lines", md)
        self.assertIn("Unit tests alone 25.0 %, browser tests alone (the server while they ran) 50.0 %", md)
        self.assertIn("| musicus-server | 4 | 25.0 | 50.0 | 75.0 |", md)
        stored = json.loads((self.root / "r.json").read_text(encoding="utf-8"))
        self.assertEqual(stored["format"], tr.FORMAT)
        self.assertEqual((stored["coverage"]["lines"], stored["coverage"]["hit"]), (4, 3))
        self.assertEqual(stored["coverage"]["sources"]["unit"]["hit"], 1)
        self.assertNotIn("files", stored["coverage"])


class Targets(unittest.TestCase):
    def test_unit_test_in_a_module(self) -> None:
        t = test("musicus-api", "ink::tests::round_trip_keeps_every_object")
        self.assertTrue(tr.target_matches("crates/musicus-api/src/ink.rs::round_trip_keeps_every_object", t))
        self.assertTrue(tr.target_matches("crates/musicus-api/src/ink.rs", t))
        self.assertFalse(tr.target_matches("crates/musicus-api/src/naming.rs::round_trip_keeps_every_object", t))
        self.assertFalse(tr.target_matches("crates/musicus-server/src/ink.rs", t))
        self.assertFalse(tr.target_matches("crates/musicus-api/src/ink.rs::other", t))

    def test_module_segment_and_mod_rs(self) -> None:
        self.assertTrue(tr.target_matches("crates/musicus-api/src/layout.rs::tests", test("musicus-api", "layout::tests::x")))
        t = test("musicus-api", "import::tests::detection")
        self.assertTrue(tr.target_matches("crates/musicus-api/src/import/mod.rs::detection", t))
        self.assertTrue(tr.target_matches("crates/musicus-api/src/lib.rs", test("musicus-api", "tests::root")))

    def test_binary_unit_tests(self) -> None:
        t = test("musicus-web::bin/musicus-web", "names::tests::part_name")
        self.assertTrue(tr.target_matches("crates/musicus-web/src/names.rs::part_name", t))

    def test_integration_test_file(self) -> None:
        t = test("musicus-server::api", "user_administration")
        self.assertTrue(tr.target_matches("crates/musicus-server/tests/api.rs::user_administration", t))
        self.assertTrue(tr.target_matches("crates/musicus-server/tests/api.rs", t))
        self.assertFalse(tr.target_matches("crates/musicus-server/tests/shutdown.rs", t))

    def test_other_paths_match_a_suite_of_that_name(self) -> None:
        t = test("e2e/smoke.mjs", "login")
        self.assertTrue(tr.target_matches("e2e/smoke.mjs", t))
        self.assertFalse(tr.target_matches("e2e/german.mjs", t))


class Requirements(unittest.TestCase):
    def item(self, rid: str, verified_by: list[str], status: str = "verified", priority: str = "must") -> dict:
        return {"id": rid, "level": 3, "status": status, "priority": priority, "title": rid,
                "links": {"verified_by": [{"target": v} for v in verified_by]}}

    def test_states_and_score(self) -> None:
        tests = {
            "musicus-api::ink::tests::a": test("musicus-api", "ink::tests::a"),
            "musicus-api::ink::tests::b": test("musicus-api", "ink::tests::b", "failed"),
        }
        items = [
            self.item("R-A", ["crates/musicus-api/src/ink.rs::a"]),
            self.item("R-B", ["crates/musicus-api/src/ink.rs::b", "crates/musicus-api/src/ink.rs::a"]),
            self.item("R-C", []),
            self.item("R-D", ["e2e/smoke.mjs"]),
            self.item("R-E", ["crates/musicus-api/src/ink.rs::renamed"], status="implemented"),
            self.item("R-F", [], status="proposed"),
            self.item("R-G", [], priority="could"),
            {"id": "F-X", "level": 2, "status": "verified", "priority": "must", "links": {}},
        ]
        req = tr.requirement_coverage(items, tests)
        states = {r["id"]: r["state"] for r in req["rows"]}
        self.assertEqual(states, {"R-A": "passing", "R-B": "failing", "R-C": "untested",
                                  "R-D": "not in this run", "R-E": "not in this run"})
        self.assertEqual((req["total"], req["tested"], req["passing"]), (5, 4, 1))
        self.assertEqual([r["unmatched"] for r in req["rows"] if r["id"] == "R-E"],
                         [["crates/musicus-api/src/ink.rs::renamed"]])


class Comparison(unittest.TestCase):
    def test_regressions_against_main(self) -> None:
        tests = {
            "s::a": test("s", "a", "failed"),
            "s::b": test("s", "b", "failed"),
            "s::c": test("s", "c", "passed"),
            "s::new": test("s", "new"),
        }
        baseline = {"format": tr.FORMAT, "commit": "abc1234def",
                    "tests": {"s::a": "passed", "s::b": "failed", "s::c": "failed", "s::gone": "passed"}}
        diff = tr.compare(tests, baseline)
        self.assertEqual(diff["newly_failing"], ["s::a"])
        self.assertEqual(diff["still_failing"], ["s::b"])
        self.assertEqual(diff["fixed"], ["s::c"])
        self.assertEqual(diff["added"], ["s::new"])
        self.assertEqual(diff["removed"], ["s::gone"])
        self.assertIsNone(tr.compare(tests, None))


class EndToEnd(unittest.TestCase):
    def test_report_and_its_json_as_the_next_baseline(self) -> None:
        d = Path(tempfile.mkdtemp())
        junit = d / "junit.xml"
        junit.write_text(JUNIT, encoding="utf-8")
        reqs = d / "requirements.json"
        reqs.write_text(json.dumps({"items": [
            {"id": "R-A", "level": 3, "status": "verified", "priority": "must", "title": "Ink",
             "links": {"verified_by": [{"target": "crates/musicus-api/src/ink.rs::round_trip_keeps_every_object"}]}},
            {"id": "R-B", "level": 3, "status": "implemented", "priority": "should", "title": "No test", "links": {}},
        ]}), encoding="utf-8")
        args = ["--junit", str(junit), "--requirements", str(reqs), "--commit", "1234567890",
                "--json", str(d / "report.json"), "--markdown", str(d / "report.md"), "--full", str(d / "full.md")]
        self.assertEqual(tr.main(args), 0)
        short = (d / "report.md").read_text(encoding="utf-8")
        full = (d / "full.md").read_text(encoding="utf-8")
        self.assertTrue(short.startswith(tr.MARKER))
        self.assertIn("**6 tests**: 3 passed, 1 failed, 1 flaky, 1 skipped", short)
        self.assertIn("### ❌ Failing (1)", short)
        self.assertIn("### Requirement coverage: 50.0 %", short)
        self.assertIn("R-B (should): No test", short)
        self.assertNotIn("All 6 tests", short)
        self.assertIn("All 6 tests", full)
        stored = json.loads((d / "report.json").read_text(encoding="utf-8"))
        self.assertEqual(stored["tests"]["musicus-api::naming::tests::keys"], "failed")
        self.assertNotIn("rows", stored["requirements"])

        # The next run, with that report as the baseline: the failing test is fixed, one is gone.
        junit.write_text(JUNIT.replace('<failure message="assertion failed">left != right</failure>', "")
                         .replace('<testcase name="layout::tests::bands_merge" classname="musicus-api" time="0.02"/>', ""),
                         encoding="utf-8")
        args = ["--junit", str(junit), "--baseline", str(d / "report.json"), "--markdown", str(d / "next.md")]
        self.assertEqual(tr.main(args), 0)
        nxt = (d / "next.md").read_text(encoding="utf-8")
        self.assertIn("compared with main `1234567`", nxt)
        self.assertIn("### ✅ Fixed (1)", nxt)
        self.assertIn("### ⚠️ Tests no longer run (1)", nxt)
        self.assertIn("`musicus-api::layout::tests::bands_merge`", nxt)

    def test_baseline_of_another_format_is_ignored(self) -> None:
        d = Path(tempfile.mkdtemp())
        junit = d / "junit.xml"
        junit.write_text(JUNIT, encoding="utf-8")
        (d / "old.json").write_text(json.dumps({"format": 0, "tests": {}}), encoding="utf-8")
        tr.main(["--junit", str(junit), "--baseline", str(d / "old.json"), "--markdown", str(d / "r.md")])
        self.assertIn("No report of main to compare with", (d / "r.md").read_text(encoding="utf-8"))


if __name__ == "__main__":
    unittest.main(verbosity=1)
