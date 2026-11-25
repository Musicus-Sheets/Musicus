#!/usr/bin/env python3
"""JUnit XML from the output of one end-to-end browser test (e2e/*.mjs), for the test report
(a-test-report, a-e2e-ci). Standard library only.

The e2e scripts print `ok - <check>` for every check that passed and stop at the first failing
one with `check failed: <check>` (or another error). Every `ok -` line becomes a passed test case
of the suite (e.g. `e2e/smoke.mjs`, the name `verified_by` uses); a failed run adds one failed
case: the failing check if the output names it, else `<suite> (run)`, with the end of the output
as the message. Checks after the failure never ran and are not listed (the report shows them as
"no longer run" against main). The same check text twice gets " (2)", " (3)", ...

Usage:
  python3 scripts/e2e-junit.py --suite e2e/smoke.mjs --log output.log --status 0 --out junit.xml
"""

from __future__ import annotations

import argparse
import re
import sys
import xml.etree.ElementTree as ET
from pathlib import Path

OK = re.compile(r"^ok - (.+?)\s*$")
FAILED = re.compile(r"check failed: (.+?)\s*$")
TAIL = 40
# Colour codes (Node, Playwright) and characters XML 1.0 does not allow (ElementTree writes them
# unescaped, which makes the file unreadable).
ANSI = re.compile(r"\x1b\[[0-9;?]*[ -/]*[@-~]")
NOT_XML = re.compile("[^\t\n\r\x20-\ud7ff\ue000-\ufffd\U00010000-\U0010ffff]")


def clean(text: str) -> str:
    return NOT_XML.sub("", ANSI.sub("", text))


def unique(name: str, seen: dict[str, int]) -> str:
    seen[name] = seen.get(name, 0) + 1
    return name if seen[name] == 1 else f"{name} ({seen[name]})"


def junit(suite: str, lines: list[str], status: int) -> ET.Element:
    seen: dict[str, int] = {}
    cases: list[ET.Element] = []
    failed_check = None
    for line in lines:
        if m := OK.match(line):
            cases.append(ET.Element("testcase", name=unique(m.group(1), seen), classname=suite, time="0"))
        elif m := FAILED.search(line):
            failed_check = m.group(1)
    if status != 0:
        name = unique(failed_check, seen) if failed_check else f"{suite} (run)"
        case = ET.Element("testcase", name=name, classname=suite, time="0")
        failure = ET.SubElement(case, "failure", message=f"exit status {status}")
        failure.text = "\n".join(lines[-TAIL:])
        cases.append(case)
    root = ET.Element("testsuites")
    element = ET.SubElement(
        root, "testsuite", name=suite, tests=str(len(cases)), failures=str(int(status != 0)), errors="0"
    )
    element.extend(cases)
    return root


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--suite", required=True, help="suite name, e.g. e2e/smoke.mjs")
    p.add_argument("--log", type=Path, required=True, help="the test's output (stdout and stderr)")
    p.add_argument("--status", type=int, required=True, help="the test's exit status")
    p.add_argument("--out", type=Path, required=True)
    args = p.parse_args(argv)
    text = args.log.read_text(encoding="utf-8", errors="replace") if args.log.exists() else ""
    lines = clean(text).splitlines()
    tree = ET.ElementTree(junit(args.suite, lines, args.status))
    ET.indent(tree)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    tree.write(args.out, encoding="utf-8", xml_declaration=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
