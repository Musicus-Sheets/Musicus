#!/usr/bin/env python3
"""Release check of a release pull request (R-OPS-040, R-OPS-018, R-OPS-035; a-release-pipeline).

    scripts/release-check.py --branch release/X.Y.Z BASE [HEAD]
    scripts/release-check.py --title-pages X.Y.Z FILE...

The first form compares the commit HEAD (the release; default HEAD) with BASE, the main branch it
is released from (their merge base, like scripts/check-db-changes.py), and fails if

  version   the branch is not named release/X.Y.Z; the workspace version (`[workspace.package]
            version` of Cargo.toml) or a musicus-* package in Cargo.lock at HEAD is not X.Y.Z; the
            version is not higher than the workspace version on BASE;
  patch     (X.Y.Z, Z > 0; R-OPS-018) a preview update exists, `migrations/X.Y.Z.sql` exists,
            anything under migrations/ changed, or X.Y is not the last line in releases.txt;
  minor     (X.Y.0) a preview update is left; the previews on BASE were not all merged into a new
  / major   `migrations/X.Y.0.sql` whose `-- @previews:` line lists exactly them (no previews on
            BASE: no such file); releases.txt did not gain exactly the line `X.Y` at its end;
            anything else under migrations/ changed.

Whether the merged update gives the structure of the previews is checked by
`scripts/db-schema.sh --check` (run it as well); a release build refuses to start with previews
(R-OPS-024), which the smoke test of the release pipeline exercises.

The second form checks the title pages of the built documentation (R-OPS-035): every HTML file
(`<p class="version">` of the title block) and the first page of every PDF (text via `pdftotext`
from poppler) must name "Musicus X.Y.Z" and must not describe a development version.

Exit code 1 with one message per problem (`::error` annotations under GitHub Actions); 0 otherwise.
Used by .github/workflows/release.yml; runs locally too, e.g.
`scripts/release-check.py --branch "$(git branch --show-current)" origin/main`.
"""

from __future__ import annotations

import argparse
import html
import importlib.util
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent


def _load(name: str, file: str):
    spec = importlib.util.spec_from_file_location(name, HERE / file)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


# The database check's git helpers and paths, and the merge script's parser of update files.
cdc = _load("check_db_changes", "check-db-changes.py")
sys.path.insert(0, str(HERE))  # release-migrations.py imports release_simplify
rm = _load("release_migrations", "release-migrations.py")

MIGRATIONS = cdc.MIGRATIONS
PREVIEWS = cdc.PREVIEWS
RELEASES = MIGRATIONS + "releases.txt"
VERSION = re.compile(r"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$")
BRANCH = re.compile(r"^release/(.+)$")


def parse_version(text: str) -> tuple[int, int, int] | None:
    m = VERSION.match(text)
    return (int(m[1]), int(m[2]), int(m[3])) if m else None


def workspace_version(cargo_toml: str) -> str | None:
    """`version` of `[workspace.package]` (the one source of the release version, a-version-source)."""
    section = None
    for line in cargo_toml.splitlines():
        line = line.strip()
        if line.startswith("["):
            section = line
        elif section == "[workspace.package]" and (m := re.match(r'^version\s*=\s*"([^"]*)"', line)):
            return m[1]
    return None


def lock_versions(cargo_lock: str) -> dict[str, str]:
    """Versions of the workspace's own packages (musicus-*) in Cargo.lock."""
    out: dict[str, str] = {}
    for block in cargo_lock.split("[[package]]")[1:]:
        name = re.search(r'^name\s*=\s*"([^"]*)"', block, re.M)
        version = re.search(r'^version\s*=\s*"([^"]*)"', block, re.M)
        if name and version and name[1].startswith("musicus-") and "source =" not in block:
            out[name[1]] = version[1]
    return out


def show(ref: str, path: str, cwd: str | None = None) -> str | None:
    r = subprocess.run(["git", "show", f"{ref}:{path}"], cwd=cwd, capture_output=True, text=True)
    return r.stdout if r.returncode == 0 else None


def release_lines(text: str | None) -> list[str]:
    """The entries `X.Y` of releases.txt, in order (comments and blank lines skipped)."""
    return [e for raw in (text or "").splitlines() if (e := raw.split("#")[0].strip())]


def listed_previews(text: str) -> list[str]:
    """The `-- @previews:` names of a release update (format checked by the merge script's parser)."""
    with tempfile.TemporaryDirectory() as tmp:
        path = Path(tmp) / "update.sql"
        path.write_text(text, encoding="utf-8")
        return rm.parse(path)["previews"]


def check(base: str, head: str, branch: str, cwd: str | None = None) -> tuple[list[tuple[str, str]], str]:
    """(path, message) for every problem, and a one-line summary of the release."""
    out: list[tuple[str, str]] = []
    m = BRANCH.match(branch)
    version = parse_version(m[1]) if m else None
    if version is None:
        return [("", f"`{branch}` is not a release branch: release branches are named release/X.Y.Z "
                     f"(for example release/1.3.0).")], ""
    x, y, z = version
    v = f"{x}.{y}.{z}"
    line = f"{x}.{y}"
    fork = cdc.git("merge-base", base, head, cwd=cwd).strip()

    # Version (a-version-source): the one place it is written, and the lock file that follows it.
    cargo = show(head, "Cargo.toml", cwd) or ""
    ws = workspace_version(cargo)
    if ws != v:
        out.append(("Cargo.toml", f"the workspace version is {ws or 'missing'}, the release branch says {v}: "
                                  f"set `[workspace.package] version = \"{v}\"` and run `cargo build`."))
    for name, lv in sorted(lock_versions(show(head, "Cargo.lock", cwd) or "").items()):
        if lv != v:
            out.append(("Cargo.lock", f"{name} has version {lv} in Cargo.lock, not {v}: run `cargo build` "
                                      f"after raising the workspace version and commit Cargo.lock."))
    before = workspace_version(show(fork, "Cargo.toml", cwd) or "")
    if before and (old := parse_version(before)) and old >= version:
        out.append(("Cargo.toml", f"{v} is not newer than the version on the base branch ({before})."))

    # Database updates (R-OPS-040).
    diff = [(s, p) for s, p in cdc.changes(base, head, cwd=cwd) if p.startswith(MIGRATIONS)]
    base_previews = cdc.previews_on(fork, cwd=cwd)
    head_previews = cdc.previews_on(head, cwd=cwd)
    base_lines = release_lines(show(fork, RELEASES, cwd))
    target = f"{MIGRATIONS}{x}.{y}.0.sql"
    if z > 0:
        kind = "patch"
        for p in head_previews:
            out.append((PREVIEWS + p, f"{v} is a patch release, but this preview update exists: a patch contains "
                                      f"no database update (R-OPS-018); release {x}.{y + 1}.0 instead."))
        if show(head, f"{MIGRATIONS}{v}.sql", cwd) is not None:
            out.append((f"{MIGRATIONS}{v}.sql", "a patch release has no database update of its own (R-OPS-018)."))
        for status, path in diff:
            out.append((path, f"changed in the patch release {v} ({status}): a patch changes no file under "
                              f"{MIGRATIONS} (R-OPS-018)."))
        head_lines = release_lines(show(head, RELEASES, cwd))
        if not head_lines or head_lines[-1] != line:
            last = head_lines[-1] if head_lines else "none"
            out.append((RELEASES, f"{v} is a patch of {line}, but the last released line is {last}: patches are "
                                  f"made of the newest line only (no back-ports)."))
        summary = f"{v}: patch release, no database update"
    else:
        kind = "major" if y == 0 else "minor"
        for p in head_previews:
            out.append((PREVIEWS + p, f"preview update left in the {kind} release {v}: run "
                                      f"`scripts/release-migrations.py {v}`, which merges every preview."))
        expected = [n[: -len(".sql")] for n in base_previews]
        added = {p for s, p in diff if s == "A"}
        if expected:
            text = show(head, target, cwd)
            if text is None:
                out.append((target, f"missing: the previews of the base branch ({', '.join(expected)}) must be "
                                    f"merged into it (`scripts/release-migrations.py {v}`)."))
            elif target not in added:
                out.append((target, "already on the base branch: a released update is never extended."))
            else:
                try:
                    listed = listed_previews(text)
                except rm.Problem as e:
                    out.append((target, f"not a valid update file: {e}"))
                else:
                    if listed != expected:
                        out.append((target, f"`-- @previews:` lists {' '.join(listed) or 'nothing'}, but the "
                                            f"pull request removes {' '.join(expected)}: it must list exactly the "
                                            f"previews of the base branch, in order."))
        elif show(head, target, cwd) is not None and target in added:
            out.append((target, "the base branch has no preview updates, so this release has no database update."))
        base_text = show(fork, RELEASES, cwd) or ""
        if base_text and not base_text.endswith("\n"):
            base_text += "\n"
        want = base_text + f"{line}\n"
        if (show(head, RELEASES, cwd) or "") != want:
            out.append((RELEASES, f"must gain exactly the line `{line}` at its end (and change nowhere else)."))
        if line in base_lines:
            out.append((RELEASES, f"{line} is already released."))
        elif base_lines and parse_version(base_lines[-1] + ".0") > version:
            out.append((RELEASES, f"{line} is older than the last released line {base_lines[-1]}."))
        allowed = {(s, p) for s, p in [("A", target), ("M", RELEASES), ("A", RELEASES)]}
        allowed |= {("D", PREVIEWS + n) for n in base_previews}
        for status, path in diff:
            if (status, path) not in allowed and not path.startswith(PREVIEWS):
                out.append((path, f"changed in the release ({status}): a release only merges the previews into "
                                  f"{target} and appends {line} to releases.txt."))
            elif path.startswith(PREVIEWS) and status != "D":
                out.append((path, f"preview added or changed in the release ({status}): previews are merged, "
                                  f"not added, by a release pull request."))
        summary = (f"{v}: {kind} release, " + (f"{len(expected)} preview(s) merged into {target}" if expected
                                                 else "no database update") + f", line {line} added")
    return out, summary


def page_text(path: Path) -> str:
    """The text of the title block (HTML) or of the first page (PDF), white space normalised."""
    if path.suffix == ".pdf":
        if not shutil.which("pdftotext"):
            raise RuntimeError("pdftotext (poppler-utils) is needed to read PDF title pages")
        text = subprocess.run(["pdftotext", "-f", "1", "-l", "1", str(path), "-"], check=True,
                              capture_output=True, text=True).stdout
    else:
        raw = path.read_text(encoding="utf-8")
        m = re.search(r'<p class="version">(.*?)</p>', raw, re.S)
        text = html.unescape(re.sub(r"<[^>]+>", "", m[1])) if m else ""
    return " ".join(text.split())


def check_title_pages(version: str, files: list[Path]) -> list[tuple[str, str]]:
    out: list[tuple[str, str]] = []
    want = re.compile(r"\bMusicus " + re.escape(version) + r"(?![\w.])")
    for f in files:
        try:
            text = page_text(f)
        except (OSError, RuntimeError, subprocess.CalledProcessError) as e:
            out.append((str(f), f"cannot read the title page: {e}"))
            continue
        if not want.search(text) or "development version" in text:
            shown = text[:200] or "no version line"
            out.append((str(f), f"the title page does not name Musicus {version} (it shows: {shown})."))
    return out


def report(found: list[tuple[str, str]]) -> None:
    gha = os.environ.get("GITHUB_ACTIONS") == "true"
    for path, msg in found:
        if gha:
            print(f"::error{' file=' + path if path else ''}::{msg}")
        print(f"error: {path + ': ' if path else ''}{msg}", file=sys.stderr)


def main(argv: list[str]) -> int:
    if argv[:1] == ["--title-pages"]:
        tp = argparse.ArgumentParser(prog="release-check.py --title-pages", description="check title pages")
        tp.add_argument("version", help="the release version X.Y.Z")
        tp.add_argument("files", nargs="+", type=Path, help="HTML and PDF files of the built documentation")
        args = tp.parse_args(argv[1:])
        found = check_title_pages(args.version, args.files)
        report(found)
        if not found:
            print(f"title pages name Musicus {args.version}: {', '.join(str(f) for f in args.files)}")
        return 1 if found else 0
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("base", help="the main branch (commit) the release is made from")
    ap.add_argument("head", nargs="?", default="HEAD", help="the release commit (default HEAD)")
    ap.add_argument("--branch", required=True, help="the release branch, release/X.Y.Z")
    args = ap.parse_args(argv)
    found, summary = check(args.base, args.head, args.branch)
    report(found)
    if found:
        return 1
    print(f"release check passed: {summary}.")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
