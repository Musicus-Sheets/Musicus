#!/usr/bin/env python3
"""Tests of scripts/e2e-junit.py and scripts/test-gate.sh (a-test-report, a-e2e-ci). Standard
library only; run directly or through `cargo test` (crates/musicus-server/tests/ci_tools.rs)."""

from __future__ import annotations

import importlib.util
import json
import os
import subprocess
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


ej = load("e2e_junit", "e2e-junit.py")
tr = load("test_report", "test-report.py")


class E2eJunit(unittest.TestCase):
    def convert(self, output: str, status: int) -> dict[str, dict]:
        with tempfile.TemporaryDirectory() as d:
            log, out = Path(d, "output.log"), Path(d, "junit.xml")
            log.write_text(output, encoding="utf-8")
            ej.main(["--suite", "e2e/smoke.mjs", "--log", str(log), "--status", str(status), "--out", str(out)])
            ET.parse(out)  # well-formed
            return tr.read_junit([out])

    def test_passed_run(self):
        tests = self.convert("server ready\nok - login works\nok - library lists 3 pieces\nsmoke test passed\n", 0)
        self.assertEqual(sorted(tests), ["e2e/smoke.mjs::library lists 3 pieces", "e2e/smoke.mjs::login works"])
        self.assertTrue(all(t["status"] == "passed" and t["suite"] == "e2e/smoke.mjs" for t in tests.values()))

    def test_failed_check_is_named(self):
        tests = self.convert("ok - login works\nError: check failed: import all green\n    at main\n", 1)
        self.assertEqual(tests["e2e/smoke.mjs::login works"]["status"], "passed")
        self.assertEqual(tests["e2e/smoke.mjs::import all green"]["status"], "failed")

    def test_other_failure_is_the_run(self):
        tests = self.convert("ok - login works\nTimeoutError: locator.click\n", 1)
        self.assertEqual(tests["e2e/smoke.mjs::e2e/smoke.mjs (run)"]["status"], "failed")

    def test_repeated_checks_stay_apart(self):
        tests = self.convert("ok - saved\nok - saved\nok - saved\n", 0)
        self.assertEqual(len(tests), 3)
        self.assertIn("e2e/smoke.mjs::saved (3)", tests)

    def test_colour_codes_and_control_characters(self):
        tests = self.convert("ok - \x1b[32mlogin\x1b[0m works\nError: check failed: bad \x08\x1b[2Kthing\n", 1)
        self.assertIn("e2e/smoke.mjs::login works", tests)
        self.assertEqual(tests["e2e/smoke.mjs::bad thing"]["status"], "failed")

    def test_missing_log_after_a_crash(self):
        with tempfile.TemporaryDirectory() as d:
            out = Path(d, "junit.xml")
            ej.main(["--suite", "e2e/german.mjs", "--log", str(Path(d, "none.log")), "--status", "2", "--out", str(out)])
            self.assertEqual(tr.read_junit([out])["e2e/german.mjs::e2e/german.mjs (run)"]["status"], "failed")


class Gate(unittest.TestCase):
    def run_gate(self, *, unit=True, doc="0", e2e=("smoke", "german"), failed=0, flaky=0, report=True) -> int:
        with tempfile.TemporaryDirectory() as d:
            results = Path(d, "results")
            if unit:
                (results / "results-unit").mkdir(parents=True)
                for f in ("nextest-ci-junit.xml", "nextest-ci-embed-junit.xml"):
                    (results / "results-unit" / f).write_text("<testsuites/>")
                if doc is not None:
                    (results / "results-unit" / "doctests-status").write_text(doc + "\n")
            for t in e2e:
                (results / f"results-e2e-{t}").mkdir(parents=True)
                (results / f"results-e2e-{t}" / f"e2e-{t}-junit.xml").write_text("<testsuites/>")
            path = Path(d, "report.json")
            if report:
                path.write_text(json.dumps({"counts": {"passed": 3, "failed": failed, "flaky": flaky}}))
            env = dict(os.environ, E2E_REQUIRED="smoke german")
            return subprocess.run(
                ["bash", str(HERE / "test-gate.sh"), str(results), str(path)], env=env, capture_output=True
            ).returncode

    def test_all_green(self):
        self.assertEqual(self.run_gate(), 0)

    def test_failed_or_flaky(self):
        self.assertEqual(self.run_gate(failed=1), 1)
        self.assertEqual(self.run_gate(flaky=2), 1)

    def test_missing_results(self):
        self.assertEqual(self.run_gate(unit=False), 1)
        self.assertEqual(self.run_gate(e2e=("smoke",)), 1)
        self.assertEqual(self.run_gate(report=False), 1)

    def test_doctests(self):
        self.assertEqual(self.run_gate(doc="101"), 1)
        self.assertEqual(self.run_gate(doc=None), 1)


if __name__ == "__main__":
    unittest.main()
