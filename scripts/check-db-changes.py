#!/usr/bin/env python3
"""Database check of a pull request (R-OPS-038, a-db-check): the database changes only through a
new preview update. Compares the commit HEAD with the base branch BASE (their merge base):

  (a) no file is added under crates/musicus-server/migrations/ outside preview/ (release updates
      X.Y.0.sql and releases.txt are written only by a release pull request), and every new file
      under preview/ is named NNNN-<topic>.sql;
  (b) no file under migrations/ that exists on the base (a released update, a merged preview,
      releases.txt) is modified, renamed or deleted;
  (c) is not checked here: scripts/db-schema.sh --check compares schema.sql with the database the
      updates create;
  (d) schema.sql does not change without a new preview (adding it when the base has none, or a
      change of its generator, e.g. its layout, is no database change);
  (e) a new preview's number is higher than every preview on BASE and differs from the other new
      previews (so development databases apply previews in the order they were merged; in the merge
      queue, a group of pull requests is checked together).

A release pull request (its branch is release/X.Y.Z) is exempt from all of these: the release
check (R-OPS-040) covers it. --list-new-previews prints the previews the pull request adds, one per
line (for the `db-change` notice, R-OPS-039), and never fails.

  scripts/check-db-changes.py [--branch NAME] [--list-new-previews] BASE [HEAD]

Exit code 1 with one message per problem; 0 otherwise. Used by .github/workflows/database.yml;
runs locally too, e.g. `scripts/check-db-changes.py --branch "$(git branch --show-current)" origin/main`.
"""

from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys

MIGRATIONS = "crates/musicus-server/migrations/"
PREVIEWS = MIGRATIONS + "preview/"
SCHEMA = "crates/musicus-server/schema.sql"
PREVIEW_NAME = re.compile(r"^(\d{4})-[a-z0-9-]+\.sql$")
RELEASE_BRANCH = re.compile(r"^release/\d+\.\d+\.\d+$")
NEW_PREVIEW = PREVIEWS + "NNNN-<topic>.sql"
# The snapshot's generator: a pull request that changes it may change schema.sql without a preview.
GENERATOR = (
    "crates/musicus-server/src/updates/schema.rs",
    "crates/musicus-server/examples/db_schema.rs",
    "scripts/db-schema.sh",
)


def git(*args: str, cwd: str | None = None) -> str:
    return subprocess.run(["git", *args], cwd=cwd, check=True, capture_output=True, text=True).stdout


def changes(base: str, head: str, cwd: str | None = None) -> list[tuple[str, str]]:
    """(status letter, path) of every file changed between the merge base and HEAD; renames are
    a deletion plus an addition."""
    fork = git("merge-base", base, head, cwd=cwd).strip()
    out = git("diff", "--name-status", "--no-renames", "-z", fork, head, cwd=cwd)
    parts = out.split("\0")
    return [(parts[i][0], parts[i + 1]) for i in range(0, len(parts) - 1, 2)]


def previews_on(ref: str, cwd: str | None = None) -> list[str]:
    """The preview files on `ref` (none if it has no preview folder)."""
    r = subprocess.run(["git", "ls-tree", "--name-only", f"{ref}:{PREVIEWS}"], cwd=cwd, capture_output=True, text=True)
    return sorted(n for n in r.stdout.splitlines() if n.endswith(".sql")) if r.returncode == 0 else []


def new_previews(diff: list[tuple[str, str]]) -> list[str]:
    return sorted(p for s, p in diff if s == "A" and p.startswith(PREVIEWS))


def problems(diff: list[tuple[str, str]], base_previews: list[str]) -> list[tuple[str, str]]:
    """(path, message) for every rule the change breaks (not for release pull requests)."""
    out: list[tuple[str, str]] = []
    added = new_previews(diff)
    for status, path in diff:
        if not path.startswith(MIGRATIONS):
            continue
        name = path[len(MIGRATIONS) :]
        if status == "A":
            if path.startswith(PREVIEWS):
                if not PREVIEW_NAME.match(path[len(PREVIEWS) :]):
                    out.append((path, f"a preview update is named {NEW_PREVIEW} (four digits, lower-case topic)."))
            else:
                out.append(
                    (
                        path,
                        f"only a release pull request (branch release/<version>) adds files under {MIGRATIONS} "
                        f"outside preview/. Add a new preview {NEW_PREVIEW} instead.",
                    )
                )
            continue
        what = {"D": "deleted", "M": "modified", "T": "modified"}.get(status, "changed")
        if name == "releases.txt":
            out.append((path, f"{what}: releases.txt changes only in a release pull request (scripts/release-migrations.py)."))
        elif status == "D":
            out.append(
                (
                    path,
                    f"{what} (or renamed): this database update is already on the base branch. Only a release pull "
                    f"request removes previews (by merging them). Restore the file; to undo its changes, add a new "
                    f"preview {NEW_PREVIEW}.",
                )
            )
        else:
            out.append(
                (
                    path,
                    f"{what}: this database update is already on the base branch, and databases may have applied it. "
                    f"Never edit it: restore it and add a new preview {NEW_PREVIEW} with the correction instead.",
                )
            )
    # Status A: the pull request that introduces schema.sql (the file is not on the base yet).
    generator = any(p in GENERATOR for _, p in diff)
    if any(p == SCHEMA and s != "A" for s, p in diff) and not added and not generator:
        out.append(
            (
                SCHEMA,
                f"schema.sql changed, but the pull request adds no preview update. The database changes only "
                f"through a new preview {NEW_PREVIEW}: add one, or restore schema.sql (scripts/db-schema.sh).",
            )
        )
    highest = max((m.group(1) for n in base_previews if (m := PREVIEW_NAME.match(n))), default=None)
    for path in added:
        m = PREVIEW_NAME.match(path[len(PREVIEWS) :])
        if m and highest is not None and m.group(1) <= highest and path[len(PREVIEWS) :] not in base_previews:
            out.append(
                (
                    path,
                    f"the base branch already has previews up to number {highest}; renumber this preview to "
                    f"{int(highest) + 1:04d} (and run scripts/db-schema.sh).",
                )
            )
    numbers = [m.group(1) for p in added if (m := PREVIEW_NAME.match(p[len(PREVIEWS) :]))]
    for number in sorted({n for n in numbers if numbers.count(n) > 1}):
        out.append((PREVIEWS, f"several new previews have the number {number}; give each its own number."))
    return out


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("base", help="the base branch (commit) of the pull request")
    ap.add_argument("head", nargs="?", default="HEAD", help="the pull request's commit (default HEAD)")
    ap.add_argument("--branch", default="", help="the pull request's head branch (release/X.Y.Z is exempt)")
    ap.add_argument("--list-new-previews", action="store_true", help="print the added previews and exit")
    args = ap.parse_args(argv)

    diff = changes(args.base, args.head)
    if args.list_new_previews:
        for p in new_previews(diff):
            print(p)
        return 0
    if RELEASE_BRANCH.match(args.branch):
        print(f"{args.branch} is a release pull request: the release check (R-OPS-040) checks its database updates.")
        return 0
    found = problems(diff, previews_on(args.base))
    gha = os.environ.get("GITHUB_ACTIONS") == "true"
    for path, msg in found:
        if gha:
            print(f"::error file={path}::{msg}")
        print(f"error: {path}: {msg}", file=sys.stderr)
    if found:
        return 1
    added = new_previews(diff)
    print(f"database check passed ({len(added)} new preview update(s){': ' + ', '.join(added) if added else ''}).")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
