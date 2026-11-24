#!/usr/bin/env python3
"""Release notes of a release, from its pull request (a-release-pipeline, RELEASING.md § 5).

    scripts/release-notes.py --version X.Y.Z [--body FILE] [--image REF]... [--missing PLATFORM]...

Reads the description of the release pull request (FILE, default standard input) and prints the
section under the heading `## Release notes` (any level from # to ###, case ignored) up to the
next heading of the same or a higher level; HTML comments are dropped. Fails (exit 1) if there is
no such section or it is empty, so the dry run on the release pull request catches missing notes.

Then it adds what the pipeline knows: one line per platform whose program could not be built or
tested (`--missing`, only the non-blocking macos-x86_64, R-OPS-042), the container images
(`--image`, the version tags pushed in stage 2) and how to check the downloads (SHA256SUMS).
Used by .github/workflows/release.yml; tested by scripts/test_release_tools.py.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

HEADING = re.compile(r"^(#{1,6})\s+(.*?)\s*#*\s*$")
NAMES = {
    "linux-x86_64": "Linux (x86_64)",
    "linux-arm64": "Linux (arm64, e.g. Raspberry Pi)",
    "windows-x86_64": "Windows (x86_64)",
    "macos-arm64": "macOS (Apple silicon)",
    "macos-x86_64": "macOS (Intel)",
}


class Missing(Exception):
    pass


def section(body: str) -> str:
    body = re.sub(r"<!--.*?-->", "", body.replace("\r\n", "\n"), flags=re.S)
    lines = body.split("\n")
    fence = False
    start = level = None
    for i, line in enumerate(lines):
        if line.lstrip().startswith(("```", "~~~")):
            fence = not fence
        m = None if fence else HEADING.match(line)
        if not m:
            continue
        if start is None:
            if len(m[1]) <= 3 and m[2].strip().lower() == "release notes":
                start, level = i + 1, len(m[1])
        elif len(m[1]) <= level:
            lines = lines[:i]
            break
    if start is None:
        raise Missing("the pull request description has no section `## Release notes`")
    text = "\n".join(lines[start:]).strip()
    if not text:
        raise Missing("the section `## Release notes` of the pull request description is empty")
    return text


def notes(body: str, version: str, images: list[str], missing: list[str]) -> str:
    out = [section(body), ""]
    if missing:
        out += ["## Not available in this release", ""]
        out += [f"- {NAMES.get(p, p)}: the program could not be built or tested for this system, so there is "
                f"no download for it in this release." for p in missing]
        out.append("")
    out += ["## Downloads", ""]
    if images:
        out.append("Container images (linux/amd64 and linux/arm64):")
        out.append("")
        out += [f"- `{ref}`" for ref in images]
        out.append("")
    out.append(f"The program archives below contain Musicus {version} with its web interface built in. "
               "Check a download with `sha256sum -c SHA256SUMS --ignore-missing` in its folder.")
    return "\n".join(out) + "\n"


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--version", required=True)
    ap.add_argument("--body", type=Path, help="the pull request description (default: standard input)")
    ap.add_argument("--image", action="append", default=[], help="a pushed image reference")
    ap.add_argument("--missing", action="append", default=[], help="a platform without a program")
    args = ap.parse_args(argv)
    body = args.body.read_text(encoding="utf-8") if args.body else sys.stdin.read()
    try:
        sys.stdout.write(notes(body, args.version, args.image, args.missing))
    except Missing as e:
        print(f"release-notes: {e}. Add it to the release pull request (RELEASING.md § 5).", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
