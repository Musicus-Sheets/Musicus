#!/usr/bin/env python3
"""Consistency checker for docs/agents (stdlib only).

    python3 docs/agents/check.py                 # structure checks
    python3 docs/agents/check.py --since REF     # + code changed since REF => docs/agents changed
    python3 docs/agents/check.py --staged        # + same for staged changes (pre-commit hook)

Escape hatch for changes that truly need no doc update:
    AGENT_DOCS_UNCHANGED="<reason>" python3 docs/agents/check.py --since REF
"""

from __future__ import annotations

import argparse
import fnmatch
import json
import os
import re
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
DOCS = REPO / "docs" / "agents"
REQ_JSON = REPO / "docs" / "requirements" / "requirements.json"
TECH_CHAPTERS = REPO / "docs" / "technical" / "chapters"

# Files that must appear in code-map.md.
MAPPED = [
    "Cargo.toml", "rustfmt.toml", "Dockerfile", ".dockerignore", "compose.yaml", ".env.example",
    "AGENTS.md", "CLAUDE.md", "crates/*", "scripts/*", "deploy/*", "e2e/*", ".githooks/*",
    ".github/workflows/*", "docs/agents/check.py",
]
NOT_MAPPED = ["crates/musicus-web/public/vendor/*", "e2e/node_modules/*", "*/target/*"]
# Changes to these require a change under docs/agents/ (--since / --staged).
CODE = [
    "crates/*", "Cargo.toml", "Dockerfile", "compose.yaml", "deploy/*", "scripts/*", "e2e/*",
    ".env.example", ".dockerignore",
]
REQ_ID = re.compile(r"\b(G-\d{3}|[FR]-[A-Z][A-Z0-9]*-\d{3})\b")
DEC_ID = re.compile(r"(?<![\w-])([da]-[a-z][a-z0-9]*(?:-[a-z0-9]+)+|d-[a-z0-9]+|a-[a-z][a-z0-9]+)(?![\w-])")
DEC_IGNORE = {"a-z"}
INV_ID = re.compile(r"\bINV-\d+\b")


def git(*args: str) -> list[str]:
    out = subprocess.run(["git", *args], cwd=REPO, capture_output=True, text=True, check=True).stdout
    return [line for line in out.splitlines() if line]


def matches(path: str, patterns: list[str]) -> bool:
    return any(fnmatch.fnmatch(path, p) for p in patterns)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--since", metavar="REF", help="compare HEAD with REF (merge base)")
    ap.add_argument("--staged", action="store_true", help="check staged changes")
    args = ap.parse_args()
    errors: list[str] = []
    warnings: list[str] = []

    docs = {p: p.read_text(encoding="utf-8") for p in sorted(DOCS.glob("*.md"))}
    docs[REPO / "AGENTS.md"] = (REPO / "AGENTS.md").read_text(encoding="utf-8")

    # 1. Code map completeness and existence.
    code_map = docs.get(DOCS / "code-map.md", "")
    mapped = set(re.findall(r"^\| `([^`]+)` \|", code_map, re.M))
    files = set(git("ls-files")) | set(git("ls-files", "--others", "--exclude-standard"))
    files = {f for f in files if (REPO / f).is_file()}
    for f in sorted(files):
        if matches(f, MAPPED) and not matches(f, NOT_MAPPED) and f not in mapped:
            errors.append(f"code-map.md: `{f}` is not listed")
    for m in sorted(mapped):
        if not (REPO / m).exists():
            errors.append(f"code-map.md: `{m}` does not exist")

    # 2. Requirement IDs exist and are not deprecated.
    if REQ_JSON.exists():
        reqs = {i["id"]: i for i in json.loads(REQ_JSON.read_text(encoding="utf-8"))["items"]}
        for path, text in docs.items():
            for rid in sorted(set(REQ_ID.findall(text))):
                if rid not in reqs:
                    errors.append(f"{path.relative_to(REPO)}: unknown requirement `{rid}`")
                elif reqs[rid]["status"] == "deprecated":
                    errors.append(f"{path.relative_to(REPO)}: `{rid}` is deprecated")
    else:
        warnings.append("docs/requirements/requirements.json missing (git submodule update --init); requirement IDs not checked")

    # 3. Decisions referenced are defined; d-* exist in the technical docs.
    defined = set(re.findall(r"^### ([da]-[a-z0-9-]+)\s*$", docs.get(DOCS / "decisions.md", ""), re.M))
    for path, text in docs.items():
        for did in sorted(set(DEC_ID.findall(text)) - DEC_IGNORE):
            if did not in defined:
                errors.append(f"{path.relative_to(REPO)}: decision `{did}` is not defined in decisions.md")
    if TECH_CHAPTERS.is_dir():
        labels = set()
        for f in TECH_CHAPTERS.glob("*.typ"):
            labels |= set(re.findall(r"<([A-Za-z0-9_-]+)>", f.read_text(encoding="utf-8")))
        for did in sorted(d for d in defined if d.startswith("d-")):
            if did not in labels:
                errors.append(f"decisions.md: `{did}` has no record `<{did}>` in docs/technical (use `a-` for agent-only decisions)")
    else:
        warnings.append("docs/technical missing; d-* labels not checked")

    # 4. Invariants referenced are defined.
    inv_defined = set(re.findall(r"^\| (INV-\d+) \|", docs.get(DOCS / "overview.md", ""), re.M))
    for path, text in docs.items():
        for inv in sorted(set(INV_ID.findall(text)) - inv_defined):
            errors.append(f"{path.relative_to(REPO)}: `{inv}` is not defined in overview.md")

    # 5. Code changed => docs/agents changed.
    changed: list[str] | None = None
    if args.since:
        changed = git("diff", "--name-only", f"{args.since}...HEAD")
    elif args.staged:
        changed = git("diff", "--cached", "--name-only")
    if changed is not None:
        code = [f for f in changed if matches(f, CODE) and not matches(f, NOT_MAPPED)]
        doc = [f for f in changed if f.startswith("docs/agents/")]
        if code and not doc:
            reason = os.environ.get("AGENT_DOCS_UNCHANGED", "").strip()
            if reason:
                warnings.append(f"code changed without docs/agents update; accepted: {reason}")
            else:
                errors.append(
                    "code changed but nothing under docs/agents/ changed:\n    "
                    + "\n    ".join(code)
                    + "\n  Update the agent docs (see AGENTS.md), or set AGENT_DOCS_UNCHANGED=\"<reason>\"."
                )

    for w in warnings:
        print(f"warning: {w}")
    for e in errors:
        print(f"error: {e}")
    if errors:
        print(f"\n{len(errors)} error(s). See AGENTS.md and docs/agents/README.md.")
        return 1
    print(f"ok: {len(mapped)} mapped files, docs/agents consistent")
    return 0


if __name__ == "__main__":
    sys.exit(main())
