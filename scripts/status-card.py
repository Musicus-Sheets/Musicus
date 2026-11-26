#!/usr/bin/env python3
"""Status images of the tests on main, for the README (a-test-status). Standard library only.

From the report.json of scripts/test-report.py it draws:
  status.svg              the card: tests per part (unit tests per crate, browser tests per script),
                          the failing tests, code coverage per crate, requirements per goal, the
                          last runs on main;
  badge-tests.svg, badge-coverage.svg, badge-requirements.svg   small badges;
and keeps history.json (the last HISTORY runs). The images follow the reader's light or dark mode.
The workflow `Tests` writes them on every run on main to the branch `status`.

Usage:
  python3 scripts/status-card.py --report report.json --gate success --commit <sha> \\
      [--history old-history.json] --out status/
  --gate   the result of the merge gate (success, failure, ...): the card is green only on success.
  Without --report (the tests could not run) the card says so.
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import sys
from pathlib import Path
from xml.sax.saxutils import escape

HISTORY = 20
W, PAD, LABEL_W, COUNT_W = 720, 24, 130, 70
GOAL_LABEL_W, GOAL_COUNT_W, GOAL_TITLE = 300, 104, 38  # title cut after GOAL_TITLE characters
PALETTE = {
    "light": {"bg": "#ffffff", "fg": "#1f2430", "muted": "#5d6575", "line": "#e1e4ea", "track": "#eceff3",
              "ok": "#2e9a5b", "bad": "#d1453b", "accent": "#3559c7", "okText": "#1d7a45", "badText": "#b3332a"},
    "dark": {"bg": "#0d1117", "fg": "#e6edf3", "muted": "#9198a1", "line": "#30363d", "track": "#21262d",
             "ok": "#3fb950", "bad": "#f85149", "accent": "#79a3ff", "okText": "#56d364", "badText": "#ff7b72"},
}
# Crates whose code mostly runs in the browser (WebAssembly), which no test run measures.
BROWSER_CODE = {"musicus-web": "browser code not measured"}


def style() -> str:
    def tokens(p: dict) -> str:
        return ";".join(f"--{k}:{v}" for k, v in p.items())
    return (
        f"<style>svg{{{tokens(PALETTE['light'])}}}"
        f"@media (prefers-color-scheme:dark){{svg{{{tokens(PALETTE['dark'])}}}}}"
        'text{font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",Helvetica,Arial,sans-serif}'
        ".t{fill:var(--fg)}.m{fill:var(--muted)}.ok{fill:var(--ok)}.bad{fill:var(--bad)}.tr{fill:var(--track)}"
        ".okt{fill:var(--okText)}.badt{fill:var(--badText)}.ln{stroke:var(--line)}.bg{fill:var(--bg)}"
        ".ac{fill:var(--accent)}.n{font-variant-numeric:tabular-nums}</style>"
    )


def e(text) -> str:
    return escape(str(text), {'"': "&quot;"})


# ---- data ------------------------------------------------------------------------------------------

def split_id(test_id: str) -> tuple[str, str]:
    """(suite, name) of a report id `<suite>::<name>`: e2e suites end in `.mjs` (their check texts
    may contain anything), Rust ids start with the crate."""
    if test_id.startswith("e2e/") and ".mjs::" in test_id:
        suite, _, name = test_id.partition(".mjs::")
        return suite + ".mjs", name
    crate, _, rest = test_id.partition("::")
    return crate, rest


def parts(report: dict) -> list[tuple[str, list[dict]]]:
    """[(kind, [{name, passed, failed}])]: unit tests per crate, browser tests per script."""
    unit: dict[str, dict] = {}
    browser: dict[str, dict] = {}
    for test_id, status in report.get("tests", {}).items():
        suite, _ = split_id(test_id)
        if suite.startswith("e2e/"):
            group, name = browser, suite.removeprefix("e2e/")
        else:
            group, name = unit, suite
        p = group.setdefault(name, {"name": name, "passed": 0, "failed": 0})
        if status != "skipped":
            p["failed" if status in ("failed", "flaky") else "passed"] += 1
    out = []
    if unit:
        out.append(("Unit tests", [unit[k] for k in sorted(unit)]))
    if browser:
        out.append(("Browser tests", [browser[k] for k in sorted(browser)]))
    return out


def failing(report: dict) -> list[tuple[str, str, str]]:
    """[(suite, name, status)] of failed and flaky tests."""
    out = []
    for test_id, status in report.get("tests", {}).items():
        if status in ("failed", "flaky"):
            suite, name = split_id(test_id)
            if name == f"{suite} (run)":
                name = "did not finish"
            out.append((suite, name, status))
    return sorted(out)


def percent(hit: int, total: int) -> float:
    return 100 * hit / total if total else 0.0


# ---- drawing ---------------------------------------------------------------------------------------

def bar(out: list, x: float, y: float, width: float, segments: list[tuple[float, str]]) -> None:
    out.append(f'<rect x="{x:.1f}" y="{y + 3:.1f}" width="{width:.1f}" height="12" rx="3" class="tr"/>')
    start = x
    for w, cls in segments:
        if w <= 0:
            continue
        w = max(3.0, w)
        out.append(f'<rect x="{start:.1f}" y="{y + 3:.1f}" width="{w:.1f}" height="12" rx="3" class="{cls}"/>')
        start += w


def heading(out: list, y: float, left: str, right: str = "", cls: str = "m") -> None:
    out.append(f'<text x="{PAD}" y="{y:.1f}" class="{cls}" font-size="11" font-weight="600" '
               f'letter-spacing=".06em">{e(left.upper())}</text>')
    if right:
        out.append(f'<text x="{W - PAD}" y="{y:.1f}" text-anchor="end" class="m" font-size="11">{e(right)}</text>')


def rule(out: list, y: float) -> None:
    out.append(f'<line x1="{PAD}" x2="{W - PAD}" y1="{y:.1f}" y2="{y:.1f}" class="ln"/>')


def right_value(out: list, y: float, text: str, bad: bool = False) -> None:
    cls, weight = ("badt", 600) if bad else ("t", 400)
    out.append(f'<text x="{W - PAD}" y="{y + 13:.1f}" text-anchor="end" class="{cls} n" font-size="12.5" '
               f'font-weight="{weight}">{e(text)}</text>')


def card(report: dict | None, gate: str, commit: str, date: str, history: list[dict]) -> str:
    out: list[str] = []
    groups = parts(report) if report else []
    all_parts = [p for _, ps in groups for p in ps]
    passed = sum(p["passed"] for p in all_parts)
    failed = sum(p["failed"] for p in all_parts)
    ok = gate == "success"
    if not report:
        pill = "no results"
    elif ok:
        pill = f"{passed} passed"
    elif failed:
        pill = f"{failed} failed · {passed} passed"
    else:
        pill = "incomplete"

    out.append(f'<text x="{PAD}" y="38" class="t" font-size="18" font-weight="600">Musicus tests</text>')
    out.append(f'<text x="{PAD}" y="58" class="m" font-size="12">main · {e(commit[:7])} · {e(date)}</text>')
    pw = len(pill) * 7.2 + 28
    out.append(f'<rect x="{W - PAD - pw:.1f}" y="22" width="{pw:.1f}" height="26" rx="13" '
               f'class="{"ok" if ok else "bad"}"/>')
    out.append(f'<text x="{W - PAD - pw / 2:.1f}" y="39.5" text-anchor="middle" font-size="12.5" '
               f'font-weight="600" fill="#fff">{"✓" if ok else "✗"} {e(pill)}</text>')
    y = 84.0

    if not report:
        out.append(f'<text x="{PAD}" y="{y:.1f}" class="t" font-size="12.5">The tests could not run on this '
                   "commit: see the workflow run.</text>")
        y += 30
    else:
        # Tests per part.
        biggest = max([p["passed"] + p["failed"] for p in all_parts] or [1])
        bar_x, bar_w = PAD + LABEL_W, W - PAD * 2 - LABEL_W - COUNT_W
        for kind, ps in groups:
            heading(out, y, kind, f"{sum(p['passed'] + p['failed'] for p in ps)} tests")
            y += 10
            for p in ps:
                n = p["passed"] + p["failed"]
                w = max(4.0, n / biggest * bar_w)
                wf = p["failed"] / biggest * bar_w if p["failed"] else 0
                out.append(f'<text x="{PAD}" y="{y + 13:.1f}" class="t" font-size="12.5">{e(p["name"])}</text>')
                bar(out, bar_x, y, bar_w, [(w - (max(4.0, wf) if wf else 0), "ok"), (wf, "bad")])
                right_value(out, y, f"{p['failed']} ✗ / {n}" if p["failed"] else str(n), bool(p["failed"]))
                y += 22
            y += 16

        # Failing tests by name.
        fails = failing(report)
        if fails:
            rule(out, y - 6)
            y += 12
            heading(out, y, "Failing", "", "badt")
            y += 8
            for suite, name, status in fails[:5]:
                mark = "✗" if status == "failed" else "~"
                tail = " (flaky)" if status == "flaky" else ""
                out.append(f'<text x="{PAD}" y="{y + 13:.1f}" class="badt" font-size="12.5">{mark}</text>')
                out.append(f'<text x="{PAD + 16}" y="{y + 13:.1f}" class="t" font-size="12.5">'
                           f'<tspan class="m">{e(suite)} ·</tspan> {e(name)}{e(tail)}</text>')
                y += 22
            if len(fails) > 5:
                out.append(f'<text x="{PAD + 16}" y="{y + 9:.1f}" class="m" font-size="11">'
                           f"and {len(fails) - 5} more</text>")
                y += 18
            y += 16

        # Code coverage per crate.
        cov = report.get("coverage")
        if cov and cov.get("lines"):
            rule(out, y - 6)
            y += 12
            heading(out, y, "Code coverage", f"{percent(cov['hit'], cov['lines']):.1f} % of all lines")
            y += 10
            for name, g in cov.get("groups", {}).items():
                pct = percent(g["hit"], g["lines"])
                out.append(f'<text x="{PAD}" y="{y + 13:.1f}" class="t" font-size="12.5">{e(name)}</text>')
                bar(out, bar_x, y, bar_w, [(pct / 100 * bar_w, "ac")])
                if name in BROWSER_CODE:
                    out.append(f'<text x="{bar_x + pct / 100 * bar_w + 8:.1f}" y="{y + 13:.1f}" class="m" '
                               f'font-size="11">{e(BROWSER_CODE[name])}</text>')
                right_value(out, y, f"{pct:.1f} %")
                y += 22
            y += 16

        # Requirements per goal.
        req = report.get("requirements") or {}
        goals = [g for g in req.get("goals", []) if g["total"] > 0]
        if goals:
            rule(out, y - 6)
            y += 12
            heading(out, y, "Requirements by goal",
                    f"{req.get('tested', 0)} of {req.get('total', 0)} built requirements have a test")
            y += 10
            gx, gw = PAD + GOAL_LABEL_W, W - PAD * 2 - GOAL_LABEL_W - GOAL_COUNT_W
            for g in goals:
                title = g["title"] if len(g["title"]) <= GOAL_TITLE else g["title"][:GOAL_TITLE - 1].rstrip() + "…"
                out.append(f'<text x="{PAD}" y="{y + 13:.1f}" class="m n" font-size="11.5">{e(g["id"])}</text>')
                out.append(f'<text x="{PAD + 44}" y="{y + 13:.1f}" class="t" font-size="12.5">{e(title)}'
                           f'<title>{e(g["title"])}</title></text>')
                bar(out, gx, y, gw, [(g["passing"] / g["total"] * gw, "ok"), (g["failing"] / g["total"] * gw, "bad")])
                text = f"{g['failing']} ✗ · {g['passing']}/{g['total']}" if g["failing"] else f"{g['passing']}/{g['total']}"
                right_value(out, y, text, bool(g["failing"]))
                y += 22
            y += 6
            lx = gx
            for cls, text in (("ok", "test passed"), ("bad", "test fails"), ("tr", "no test in this report")):
                out.append(f'<rect x="{lx:.1f}" y="{y:.1f}" width="10" height="10" rx="2" class="{cls}"/>')
                out.append(f'<text x="{lx + 15:.1f}" y="{y + 9:.1f}" class="m" font-size="11">{e(text)}</text>')
                lx += 15 + len(text) * 6 + 18
            y += 32

    # The last runs on main.
    rule(out, y - 6)
    y += 14
    heading(out, y + 9, f"Last {len(history)} runs" if len(history) != 1 else "Last run")
    sq, gap = 14, 4
    hx = W - PAD - len(history) * (sq + gap) + gap
    for i, h in enumerate(history):
        fade = "" if i == len(history) - 1 else ' opacity=".75"'
        out.append(f'<rect x="{hx + i * (sq + gap):.1f}" y="{y - 2:.1f}" width="{sq}" height="{sq}" rx="3" '
                   f'class="{"ok" if h.get("ok") else "bad"}"{fade}><title>{e(h.get("commit", "")[:7])} '
                   f'{e(h.get("date", ""))}</title></rect>')
    if len(history) > 1:
        out.append(f'<text x="{hx - 10:.1f}" y="{y + 9:.1f}" text-anchor="end" class="m" font-size="11">'
                   "older → newest</text>")
    y += 30

    h = round(y)
    return (f'<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{h}" viewBox="0 0 {W} {h}" role="img" '
            f'aria-label="Musicus tests: {e(pill)}">{style()}'
            f'<rect x=".5" y=".5" width="{W - 1}" height="{h - 1}" rx="10" class="bg ln" stroke-width="1"/>'
            + "".join(out) + "</svg>\n")


def badge(label: str, value: str, state: str) -> str:
    lw, vw = len(label) * 6.6 + 16, len(value) * 6.8 + 16
    width, cls = lw + vw, {"ok": "ok", "bad": "bad"}.get(state, "ac")
    return (f'<svg xmlns="http://www.w3.org/2000/svg" width="{width:.0f}" height="22" viewBox="0 0 {width:.1f} 22" '
            f'role="img" aria-label="{e(label)}: {e(value)}">{style()}'
            f'<rect width="{width:.1f}" height="22" rx="4" fill="#555c66"/>'
            f'<rect x="{lw:.1f}" width="{vw:.1f}" height="22" rx="4" class="{cls}"/>'
            f'<rect x="{lw:.1f}" width="6" height="22" class="{cls}"/>'
            f'<text x="{lw / 2:.1f}" y="15" text-anchor="middle" font-size="11.5" fill="#fff">{e(label)}</text>'
            f'<text x="{lw + vw / 2:.1f}" y="15" text-anchor="middle" font-size="11.5" font-weight="600" '
            f'fill="#fff">{e(value)}</text></svg>\n')


def badges(report: dict | None, gate: str) -> dict[str, str]:
    if not report:
        return {"badge-tests.svg": badge("tests", "no results", "bad"),
                "badge-coverage.svg": badge("coverage", "unknown", "info"),
                "badge-requirements.svg": badge("requirements", "unknown", "info")}
    c = report.get("counts", {})
    bad = c.get("failed", 0) + c.get("flaky", 0)
    if gate == "success":
        tests = badge("tests", f"{c.get('passed', 0)} passed", "ok")
    else:
        tests = badge("tests", f"{bad} failed" if bad else "incomplete", "bad")
    cov = report.get("coverage") or {}
    req = report.get("requirements") or {}
    return {
        "badge-tests.svg": tests,
        "badge-coverage.svg": badge("coverage", f"{percent(cov.get('hit', 0), cov.get('lines', 0)):.1f} %"
                                    if cov.get("lines") else "unknown", "info"),
        "badge-requirements.svg": badge("requirements", f"{round(percent(req.get('tested', 0), req.get('total', 0)))} % tested"
                                        if req.get("total") else "unknown", "info"),
    }


# ---- main ------------------------------------------------------------------------------------------

def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--report", type=Path, help="report.json of scripts/test-report.py (missing: no results)")
    p.add_argument("--gate", required=True, help="result of the merge gate: success, failure, ...")
    p.add_argument("--commit", required=True)
    p.add_argument("--history", type=Path, help="history.json of the previous run (optional)")
    p.add_argument("--date", help="date shown (default: today, UTC)")
    p.add_argument("--out", type=Path, required=True)
    args = p.parse_args(argv)

    report = None
    if args.report and args.report.exists():
        report = json.loads(args.report.read_text(encoding="utf-8"))
    date = args.date or dt.datetime.now(dt.timezone.utc).strftime("%-d %b %Y")
    history: list[dict] = []
    if args.history and args.history.exists():
        try:
            history = [h for h in json.loads(args.history.read_text(encoding="utf-8")).get("runs", []) if isinstance(h, dict)]
        except (ValueError, AttributeError):
            print("history.json unreadable: starting a new history", file=sys.stderr)
    # The same commit again (a re-run) replaces its entry.
    history = [h for h in history if h.get("commit") != args.commit]
    history.append({"commit": args.commit, "date": date, "ok": args.gate == "success"})
    history = history[-HISTORY:]

    args.out.mkdir(parents=True, exist_ok=True)
    (args.out / "status.svg").write_text(card(report, args.gate, args.commit, date, history), encoding="utf-8")
    for name, svg in badges(report, args.gate).items():
        (args.out / name).write_text(svg, encoding="utf-8")
    (args.out / "history.json").write_text(json.dumps({"runs": history}, indent=1) + "\n", encoding="utf-8")
    (args.out / "README.md").write_text(
        "# Test status of Musicus\n\nWritten by the workflow `Tests` (.github/workflows/tests.yml) after every run "
        "on `main`, replacing the previous commit. Do not edit: changes are overwritten by the next run.\n",
        encoding="utf-8")
    return 0


if __name__ == "__main__":
    sys.exit(main())
