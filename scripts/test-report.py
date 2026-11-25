#!/usr/bin/env python3
"""Test report for CI (a-test-report): every test with its state, regressions against main, code
coverage and requirement coverage. Standard library only.

Inputs:
  --junit FILE     JUnit XML (cargo-nextest; later also e2e). Repeatable. A test's id is
                   `<testsuite>::<testcase>`, e.g. `musicus-server::api::user_administration`.
  --lcov FILE      lcov coverage (cargo llvm-cov). Optional.
  --requirements FILE   docs/requirements/requirements.json. Optional.
  --baseline FILE  report.json of the last run on main. Optional; without it there is no comparison.
  --root DIR       repository root, to make lcov paths relative (default: current directory).
Outputs:
  --json FILE      this run's report.json (the next run's baseline).
  --markdown FILE  the short report (PR comment): counts, regressions, coverage, requirements.
  --full FILE      the same plus the list of every test (job summary).
Exit status 0 unless the inputs cannot be read: failing tests fail the test step itself.

Usage:
  python3 scripts/test-report.py --junit target/nextest/ci/junit.xml --lcov target/lcov.info \\
      --requirements docs/requirements/requirements.json --json report.json --markdown report.md
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import xml.etree.ElementTree as ET
from pathlib import Path

MARKER = "<!-- musicus-test-report -->"
FORMAT = 1
# Status order for the counts and the list.
STATES = ("failed", "flaky", "passed", "skipped")
ICON = {"passed": "✅", "failed": "❌", "flaky": "⚠️", "skipped": "⏭️"}
# Requirements the score is about: level 3, must/should, said to be built.
SCORED_STATUS = ("implemented", "verified")
SCORED_PRIORITY = ("must", "should")


# ---- inputs ------------------------------------------------------------------------------------

def read_junit(paths: list[Path]) -> dict[str, dict]:
    """Test id -> {suite, name, status, time}. Later files win for the same id."""
    tests: dict[str, dict] = {}
    for path in paths:
        root = ET.parse(path).getroot()
        suites = [root] if root.tag == "testsuite" else root.iter("testsuite")
        for suite in suites:
            suite_name = suite.get("name", "")
            for case in suite.iter("testcase"):
                name = case.get("name", "")
                children = {child.tag for child in case}
                if children & {"failure", "error"}:
                    status = "failed"
                elif "skipped" in children:
                    status = "skipped"
                elif children & {"flakyFailure", "flakyError", "rerunFailure", "rerunError"}:
                    status = "flaky"
                else:
                    status = "passed"
                tests[f"{suite_name}::{name}"] = {
                    "suite": suite_name,
                    "name": name,
                    "status": status,
                    "time": float(case.get("time") or 0),
                }
    return tests


def read_lcov(path: Path, root: Path) -> dict[str, dict]:
    """Relative source path -> {lines, hit}. Files outside the repository (dependencies) are left out."""
    files: dict[str, dict] = {}
    current = None
    root = root.resolve()
    for line in path.read_text(encoding="utf-8").splitlines():
        if line.startswith("SF:"):
            source = Path(line[3:].strip())
            try:
                current = source.resolve().relative_to(root).as_posix() if source.is_absolute() else source.as_posix()
            except ValueError:
                current = None
            if current is not None:
                files.setdefault(current, {"lines": 0, "hit": 0})
        elif current is not None and line.startswith("DA:"):
            # DA:<line>,<count>[,<checksum>]: count the lines ourselves (LF/LH are optional).
            parts = line[3:].split(",")
            files[current]["lines"] += 1
            if len(parts) > 1 and parts[1].strip() not in ("", "0"):
                files[current]["hit"] += 1
        elif line == "end_of_record":
            current = None
    return {f: v for f, v in files.items() if v["lines"]}


def group_of(path: str) -> str:
    """`crates/musicus-api/src/x.rs` -> `musicus-api`; anything else -> its first folder."""
    parts = path.split("/")
    if len(parts) > 2 and parts[0] == "crates":
        return parts[1]
    return parts[0] if len(parts) > 1 else "(root)"


def coverage_summary(files: dict[str, dict]) -> dict:
    groups: dict[str, dict] = {}
    for path, v in files.items():
        g = groups.setdefault(group_of(path), {"lines": 0, "hit": 0})
        g["lines"] += v["lines"]
        g["hit"] += v["hit"]
    lines = sum(v["lines"] for v in files.values())
    hit = sum(v["hit"] for v in files.values())
    return {"lines": lines, "hit": hit, "groups": dict(sorted(groups.items())), "files": dict(sorted(files.items()))}


def percent(hit: int, total: int) -> float | None:
    return round(100 * hit / total, 1) if total else None


# ---- requirements --------------------------------------------------------------------------------

def target_matches(target: str, test: dict) -> bool:
    """Whether the `verified_by` target `path[::name]` names this test of the run.

    crates/<c>/src/<m>.rs[::t]   unit tests of crate c (or its binaries) in module m (`mod.rs`,
                                 `lib.rs`, `main.rs` name their folder / the crate root)
    crates/<c>/tests/<f>.rs[::t] the integration test binary `<c>::<f>`
    any other path[::t]          a test suite named exactly like the path (e.g. e2e JUnit)
    `t` matches the last segment of the test name or any module segment (`layout.rs::tests`).
    """
    path, _, wanted = target.partition("::")
    suite, name = test["suite"], test["name"]
    parts = path.split("/")
    if len(parts) >= 4 and parts[0] == "crates" and parts[2] == "src" and path.endswith(".rs"):
        crate = parts[1]
        if suite != crate and not suite.startswith(f"{crate}::bin/"):
            return False
        module = parts[3:]
        module[-1] = module[-1][:-3]
        if module[-1] in ("mod", "lib", "main"):
            module = module[:-1]
        prefix = "::".join(module)
        if prefix and not name.startswith(prefix + "::"):
            return False
    elif len(parts) == 4 and parts[0] == "crates" and parts[2] == "tests" and path.endswith(".rs"):
        if suite != f"{parts[1]}::{parts[3][:-3]}":
            return False
    elif suite != path:
        return False
    if not wanted:
        return True
    segments = name.split("::")
    return wanted == segments[-1] or wanted in segments[:-1] or wanted == name


def requirement_coverage(items: list[dict], tests: dict[str, dict]) -> dict:
    scored = [
        i for i in items
        if i.get("level") == 3 and i.get("status") in SCORED_STATUS and i.get("priority") in SCORED_PRIORITY
    ]
    rows = []
    for item in sorted(scored, key=lambda i: i["id"]):
        targets = [link["target"] for link in item.get("links", {}).get("verified_by", [])]
        matched: dict[str, str] = {}
        unmatched = []
        for target in targets:
            hits = [t for t in tests.values() if target_matches(target, t)]
            if hits:
                for t in hits:
                    matched[f"{t['suite']}::{t['name']}"] = t["status"]
            elif "::" in target and target.startswith("crates/"):
                # Names a Rust test that this run did not find (renamed, removed or ignored).
                unmatched.append(target)
        states = set(matched.values())
        if not targets:
            state = "untested"
        elif "failed" in states:
            state = "failing"
        elif matched and states <= {"passed", "flaky"}:
            state = "passing"
        else:
            # Only references to tests that are not in this run's JUnit files (other scripts).
            state = "not in this run"
        rows.append({
            "id": item["id"], "title": item.get("title", ""), "priority": item.get("priority"),
            "state": state, "tests": len(matched), "unmatched": unmatched,
        })
    total = len(rows)
    tested = sum(1 for r in rows if r["state"] != "untested")
    passing = sum(1 for r in rows if r["state"] == "passing")
    return {"total": total, "tested": tested, "passing": passing, "rows": rows}


# ---- comparison ----------------------------------------------------------------------------------

def compare(tests: dict[str, dict], baseline: dict | None) -> dict | None:
    if not baseline:
        return None
    before = baseline.get("tests", {})
    now = {k: v["status"] for k, v in tests.items()}
    ok = ("passed", "flaky")
    return {
        "commit": baseline.get("commit", ""),
        "newly_failing": sorted(k for k, s in now.items() if s == "failed" and before.get(k) in ok),
        "still_failing": sorted(k for k, s in now.items() if s == "failed" and before.get(k) == "failed"),
        "fixed": sorted(k for k, s in now.items() if s in ok and before.get(k) == "failed"),
        "added": sorted(k for k in now if k not in before),
        "removed": sorted(k for k in before if k not in now),
        "coverage": baseline.get("coverage"),
        "requirements": baseline.get("requirements"),
    }


# ---- output --------------------------------------------------------------------------------------

def counts(tests: dict[str, dict]) -> dict[str, int]:
    c = {s: 0 for s in STATES}
    for t in tests.values():
        c[t["status"]] += 1
    return c


def delta(now: float | None, before: float | None) -> str:
    if now is None or before is None:
        return ""
    d = round(now - before, 1)
    return " (±0.0 pp)" if d == 0 else f" ({d:+.1f} pp)"


def code_list(ids: list[str], limit: int = 30) -> list[str]:
    out = [f"- `{i}`" for i in ids[:limit]]
    if len(ids) > limit:
        out.append(f"- … and {len(ids) - limit} more")
    return out


def render(report: dict, diff: dict | None, full: bool) -> str:
    tests = report["tests_detail"]
    c = report["counts"]
    total = sum(c.values())
    lines = [MARKER, "## Test report", ""]
    summary = ", ".join(f"{c[s]} {s}" for s in ("passed", "failed", "flaky", "skipped") if c[s] or s in ("passed", "failed"))
    head = f"**{total} tests**: {summary}"
    if report.get("commit"):
        head += f" · commit `{report['commit'][:7]}`"
    if diff:
        head += f" · compared with main `{diff['commit'][:7]}`" if diff["commit"] else " · compared with main"
    lines += [head, ""]

    # Regressions first.
    if diff is None:
        lines += ["No report of main to compare with (first run, or its artifact expired).", ""]
    else:
        if diff["newly_failing"]:
            lines += [f"### ❌ Newly failing ({len(diff['newly_failing'])})", ""] + code_list(diff["newly_failing"]) + [""]
        if diff["removed"]:
            lines += [f"### ⚠️ Tests no longer run ({len(diff['removed'])})", "",
                      "Removed, renamed or ignored since main; intended?", ""] + code_list(diff["removed"]) + [""]
        if diff["still_failing"]:
            lines += [f"### Still failing ({len(diff['still_failing'])})", ""] + code_list(diff["still_failing"]) + [""]
        if diff["fixed"]:
            lines += [f"### ✅ Fixed ({len(diff['fixed'])})", ""] + code_list(diff["fixed"]) + [""]
        if diff["added"]:
            lines += [f"New tests: {len(diff['added'])}.", ""]
        if not (diff["newly_failing"] or diff["removed"] or diff["still_failing"]):
            lines += ["No regressions against main.", ""]
    failing_now = sorted(k for k, t in tests.items() if t["status"] == "failed")
    if failing_now and diff is None:
        lines += [f"### ❌ Failing ({len(failing_now)})", ""] + code_list(failing_now) + [""]
    flaky = sorted(k for k, t in tests.items() if t["status"] == "flaky")
    if flaky:
        lines += [f"### ⚠️ Flaky: failed first, passed on the retry ({len(flaky)})", ""] + code_list(flaky) + [""]

    cov = report.get("coverage")
    if cov:
        before = (diff or {}).get("coverage") or {}
        pct = percent(cov["hit"], cov["lines"])
        lines += [f"### Code coverage: {pct} % of lines{delta(pct, percent(before.get('hit', 0), before.get('lines', 0)) if before else None)}", "",
                  "| Part | Lines | Covered | % |", "| --- | ---: | ---: | ---: |"]
        for name, g in cov["groups"].items():
            b = before.get("groups", {}).get(name)
            p = percent(g["hit"], g["lines"])
            lines.append(f"| {name} | {g['lines']} | {g['hit']} | {p}{delta(p, percent(b['hit'], b['lines']) if b else None)} |")
        lines.append("")
        if "musicus-web" in cov["groups"]:
            lines += ["`musicus-web` counts only what its host tests reach (names, state logic); its pages and the "
                      "pdf.js bridge run in the browser and are tested end to end, not measured here.", ""]
        if full:
            worst = sorted(cov["files"].items(), key=lambda kv: (kv[1]["hit"] / kv[1]["lines"], -kv[1]["lines"]))
            lines += ["<details><summary>Files by coverage (lowest first)</summary>", "",
                      "| File | Lines | Covered | % |", "| --- | ---: | ---: | ---: |"]
            lines += [f"| `{f}` | {v['lines']} | {v['hit']} | {percent(v['hit'], v['lines'])} |" for f, v in worst]
            lines += ["", "</details>", ""]

    req = report.get("requirements")
    if req and req["total"]:
        before = (diff or {}).get("requirements") or {}
        pct = percent(req["tested"], req["total"])
        bpct = percent(before.get("tested", 0), before.get("total", 0)) if before else None
        failing = sum(1 for r in req["rows"] if r["state"] == "failing")
        absent = req["tested"] - req["passing"] - failing
        lines += [f"### Requirement coverage: {pct} %{delta(pct, bpct)}", "",
                  f"{req['tested']} of {req['total']} built must/should requirements (level 3, implemented or "
                  f"verified) name a test (`verified_by`). In this run: the named tests (Rust and browser tests) "
                  f"ran and passed for {req['passing']}, one of them failed for {failing}, and {absent} name only "
                  "tests that are not in this report (other scripts).", ""]
        groups = {
            "untested": "Without a test",
            "failing": "❌ With a failing test",
        }
        for state, title in groups.items():
            rows = [r for r in req["rows"] if r["state"] == state]
            if rows:
                lines += [f"<details><summary>{title} ({len(rows)})</summary>", ""]
                lines += [f"- {r['id']} ({r['priority']}): {r['title']}" for r in rows]
                lines += ["", "</details>", ""]
        unmatched = [(r["id"], u) for r in req["rows"] for u in r["unmatched"]]
        if unmatched:
            lines += [f"<details><summary>`verified_by` tests not found in this run ({len(unmatched)})</summary>", ""]
            lines += [f"- {rid}: `{u}`" for rid, u in unmatched]
            lines += ["", "</details>", ""]

    if full:
        lines += [f"<details><summary>All {total} tests</summary>", "",
                  "| | Test | Time (s) |", "| --- | --- | ---: |"]
        order = {s: i for i, s in enumerate(STATES)}
        for key, t in sorted(tests.items(), key=lambda kv: (order[kv[1]["status"]], kv[0])):
            lines.append(f"| {ICON[t['status']]} | `{key}` | {t['time']:.2f} |")
        lines += ["", "</details>", ""]
    return "\n".join(lines).rstrip() + "\n"


def build(args: argparse.Namespace) -> tuple[dict, dict | None]:
    tests = read_junit(args.junit)
    report: dict = {
        "format": FORMAT,
        "commit": args.commit or "",
        "counts": counts(tests),
        "tests": {k: v["status"] for k, v in sorted(tests.items())},
        "tests_detail": tests,
    }
    if args.lcov:
        report["coverage"] = coverage_summary(read_lcov(args.lcov, args.root))
    if args.requirements:
        items = json.loads(args.requirements.read_text(encoding="utf-8"))["items"]
        report["requirements"] = requirement_coverage(items, tests)
    baseline = None
    if args.baseline and args.baseline.exists():
        baseline = json.loads(args.baseline.read_text(encoding="utf-8"))
        if baseline.get("format") != FORMAT:
            print(f"baseline format {baseline.get('format')} != {FORMAT}: not compared", file=sys.stderr)
            baseline = None
    return report, compare(tests, baseline)


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--junit", type=Path, action="append", required=True)
    p.add_argument("--lcov", type=Path)
    p.add_argument("--requirements", type=Path)
    p.add_argument("--baseline", type=Path)
    p.add_argument("--root", type=Path, default=Path.cwd())
    p.add_argument("--commit", default="")
    p.add_argument("--json", type=Path)
    p.add_argument("--markdown", type=Path)
    p.add_argument("--full", type=Path)
    args = p.parse_args(argv)
    report, diff = build(args)
    if args.json:
        stored = {k: v for k, v in report.items() if k != "tests_detail"}
        if "requirements" in stored:
            stored["requirements"] = {k: v for k, v in stored["requirements"].items() if k != "rows"}
        if "coverage" in stored:
            stored["coverage"] = {k: v for k, v in stored["coverage"].items() if k != "files"}
        args.json.write_text(json.dumps(stored, indent=1, sort_keys=True) + "\n", encoding="utf-8")
    if args.markdown:
        args.markdown.write_text(render(report, diff, full=False), encoding="utf-8")
    if args.full:
        args.full.write_text(render(report, diff, full=True), encoding="utf-8")
    if not (args.markdown or args.full or args.json):
        sys.stdout.write(render(report, diff, full=False))
    return 0


if __name__ == "__main__":
    sys.exit(main())
