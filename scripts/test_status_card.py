#!/usr/bin/env python3
"""Tests of scripts/status-card.py and of the goals in scripts/test-report.py (a-test-status).
Standard library only; run directly or through `cargo test` (crates/musicus-server/tests/ci_tools.rs)."""

from __future__ import annotations

import importlib.util
import json
import tempfile
import unittest
import xml.etree.ElementTree as ET
from pathlib import Path

HERE = Path(__file__).resolve().parent


def load(name: str, file: str):
    spec = importlib.util.spec_from_file_location(name, HERE / file)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


sc = load("status_card", "status-card.py")
tr = load("test_report", "test-report.py")

REPORT = {
    "format": 2,
    "counts": {"passed": 4, "failed": 1, "flaky": 0, "skipped": 1},
    "tests": {
        "musicus-api::ink::tests::round_trip": "passed",
        "musicus-server::api::user_administration": "passed",
        "musicus-server::api::ignored": "skipped",
        "e2e/smoke.mjs::login works": "passed",
        "e2e/smoke.mjs::import all green": "failed",
        "e2e/german.mjs::date 24.12.2026": "passed",
    },
    "coverage": {"lines": 100, "hit": 70, "groups": {"musicus-api": {"lines": 50, "hit": 45},
                                                      "musicus-web": {"lines": 50, "hit": 25}}},
    "requirements": {"total": 4, "tested": 3, "passing": 2, "goals": [
        {"id": "G-001", "title": "Central sheet music library", "total": 3, "passing": 2, "failing": 1},
        {"id": "G-008", "title": "Maintainable technology", "total": 1, "passing": 0, "failing": 0},
    ]},
}


class Parts(unittest.TestCase):
    def test_unit_per_crate_browser_per_script(self):
        groups = dict(sc.parts(REPORT))
        self.assertEqual([p["name"] for p in groups["Unit tests"]], ["musicus-api", "musicus-server"])
        smoke = [p for p in groups["Browser tests"] if p["name"] == "smoke.mjs"][0]
        self.assertEqual((smoke["passed"], smoke["failed"]), (1, 1))
        server = [p for p in groups["Unit tests"] if p["name"] == "musicus-server"][0]
        self.assertEqual(server["passed"], 1)  # the skipped test is not counted

    def test_failing_names(self):
        report = dict(REPORT, tests={**REPORT["tests"], "e2e/outage.mjs::e2e/outage.mjs (run)": "failed"})
        self.assertIn(("e2e/outage.mjs", "did not finish", "failed"), sc.failing(report))
        self.assertIn(("e2e/smoke.mjs", "import all green", "failed"), sc.failing(report))

    def test_check_text_with_colons(self):
        self.assertEqual(sc.split_id("e2e/smoke.mjs::a::b c"), ("e2e/smoke.mjs", "a::b c"))
        self.assertEqual(sc.split_id("musicus-server::api::x"), ("musicus-server", "api::x"))


class Images(unittest.TestCase):
    def run_card(self, report, gate, history=None) -> Path:
        d = Path(tempfile.mkdtemp())
        args = ["--gate", gate, "--commit", "abcdef1234", "--date", "26 Nov 2026", "--out", str(d / "out")]
        if report is not None:
            (d / "report.json").write_text(json.dumps(report))
            args += ["--report", str(d / "report.json")]
        if history is not None:
            (d / "history.json").write_text(json.dumps({"runs": history}))
            args += ["--history", str(d / "history.json")]
        sc.main(args)
        return d / "out"

    def test_files_are_valid_svg(self):
        out = self.run_card(REPORT, "failure")
        for name in ("status.svg", "badge-tests.svg", "badge-coverage.svg", "badge-requirements.svg"):
            ET.parse(out / name)
        card = (out / "status.svg").read_text()
        self.assertIn("1 failed", card)
        self.assertIn("import all green", card)
        self.assertIn("G-008", card)  # built but untested: shown
        self.assertIn("prefers-color-scheme:dark", card)

    def test_green_only_with_a_green_gate(self):
        self.assertIn("passed", (self.run_card(REPORT, "success") / "badge-tests.svg").read_text())
        self.assertIn("1 failed", (self.run_card(REPORT, "failure") / "badge-tests.svg").read_text())

    def test_without_report(self):
        out = self.run_card(None, "failure")
        self.assertIn("could not run", (out / "status.svg").read_text())
        ET.parse(out / "status.svg")

    def test_history_keeps_the_last_runs_and_replaces_a_rerun(self):
        old = [{"commit": f"c{i}", "date": "", "ok": True} for i in range(25)] + [
            {"commit": "abcdef1234", "date": "", "ok": False}]
        runs = json.loads((self.run_card(REPORT, "success", old) / "history.json").read_text())["runs"]
        self.assertEqual(len(runs), sc.HISTORY)
        self.assertEqual(runs[-1], {"commit": "abcdef1234", "date": "26 Nov 2026", "ok": True})
        self.assertEqual(sum(r["commit"] == "abcdef1234" for r in runs), 1)

    def test_unreadable_history_starts_again(self):
        d = Path(tempfile.mkdtemp())
        (d / "history.json").write_text("not json")
        sc.main(["--gate", "success", "--commit", "x", "--history", str(d / "history.json"), "--out", str(d / "o")])
        self.assertEqual(len(json.loads((d / "o" / "history.json").read_text())["runs"]), 1)


class Goals(unittest.TestCase):
    ITEMS = [
        {"id": "G-001", "level": 1, "title": "Library", "links": {}},
        {"id": "G-007", "level": 1, "title": "Offline", "links": {}},
        {"id": "G-008", "level": 1, "title": "Maintainable", "links": {}},
        {"id": "F-LIB-001", "level": 2, "links": {"refines": [{"target": "G-001"}, {"target": "G-008"}]}},
        {"id": "F-OFF-001", "level": 2, "links": {"refines": [{"target": "G-007"}]}},
        {"id": "R-LIB-001", "level": 3, "status": "implemented", "priority": "must",
         "links": {"refines": [{"target": "F-LIB-001"}], "verified_by": [{"target": "e2e/smoke.mjs"}]}},
        {"id": "R-LIB-002", "level": 3, "status": "verified", "priority": "should",
         "links": {"refines": [{"target": "F-LIB-001"}]}},
        {"id": "R-OFF-001", "level": 3, "status": "accepted", "priority": "must",
         "links": {"refines": [{"target": "F-OFF-001"}], "verified_by": [{"target": "e2e/smoke.mjs"}]}},
    ]

    def test_goals_through_features_built_requirements_only(self):
        tests = {"e2e/smoke.mjs::x": {"suite": "e2e/smoke.mjs", "name": "x", "status": "failed", "time": 0}}
        goals = {g["id"]: g for g in tr.requirement_coverage(self.ITEMS, tests)["goals"]}
        self.assertNotIn("G-007", goals)  # only an accepted requirement: nothing built
        self.assertEqual(goals["G-001"], {"id": "G-001", "title": "Library", "total": 2, "passing": 0, "failing": 1})
        self.assertEqual(goals["G-008"]["total"], 2)  # a requirement serving two goals counts for both


if __name__ == "__main__":
    unittest.main()
