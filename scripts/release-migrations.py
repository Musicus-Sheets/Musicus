#!/usr/bin/env python3
"""Merge the preview database updates into the release update (a-preview-migrations).

    scripts/release-migrations.py 1.3.0            # in the release pull request (branch release/1.3.0)
    scripts/release-migrations.py --migrations DIR 1.3.0

For a minor or major release X.Y.0:
  * all `migrations/preview/*.sql` (in name order) are merged into `migrations/X.Y.0.sql`:
    their additive, rebuild and removal sections are concatenated per section, the header
    line `-- @previews:` lists them (the runner then records the release update as applied on
    databases that applied all of them, without running it again, R-OPS-023);
  * the preview files are deleted;
  * the line `X.Y` is appended to `migrations/releases.txt` (release order for the window).
An existing `X.Y.0.sql` that is not released yet (the 1.0.0 baseline before the first release)
gets the previews appended. A patch release X.Y.Z (Z > 0) must have no previews (R-OPS-018);
the script then changes nothing.

The update file format is parsed like `crates/musicus-server/src/updates/file.rs`; keep both in step.
Exit status 1 with a message on any problem; nothing is written then.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

MARKERS = ["-- @additive", "-- @rebuild", "-- @removal"]
PREVIEWS = "-- @previews:"
PREVIEW_NAME = re.compile(r"^\d{4}-[a-z0-9-]+$")
DEFAULT_DIR = Path(__file__).resolve().parents[1] / "crates" / "musicus-server" / "migrations"


class Problem(Exception):
    pass


def parse(path: Path) -> dict:
    """Header comment lines, listed previews and the three section bodies of an update file."""
    out = {"header": [], "previews": [], "sections": ["", "", ""]}
    current = None
    for n, raw in enumerate(path.read_text(encoding="utf-8").splitlines(keepends=True), 1):
        line = raw.strip()
        where = f"{path.name}:{n}"
        if line in MARKERS:
            i = MARKERS.index(line)
            if current is None and i != 0:
                raise Problem(f"{where}: the first section must be `-- @additive`")
            if current is not None and current >= i:
                raise Problem(f"{where}: sections must appear once each, in the order additive, rebuild, removal")
            current = i
        elif line.startswith(PREVIEWS):
            if current is not None or out["previews"]:
                raise Problem(f"{where}: `-- @previews:` belongs once into the header")
            out["previews"] = [p for p in re.split(r"[ ,]+", line[len(PREVIEWS):]) if p]
            if not out["previews"]:
                raise Problem(f"{where}: `-- @previews:` lists no preview")
        elif line.startswith("-- @"):
            raise Problem(f"{where}: unknown marker `{line}`")
        elif current is None:
            if line and not line.startswith("--"):
                raise Problem(f"{where}: SQL before `-- @additive`")
            out["header"].append(raw if raw.endswith("\n") else raw + "\n")
        else:
            out["sections"][current] += raw if raw.endswith("\n") else raw + "\n"
    if current is None:
        raise Problem(f"{path.name}: no `-- @additive` section")
    return out


def is_empty(sql: str) -> bool:
    return all(not l.strip() or l.strip().startswith("--") for l in sql.splitlines())


def read_releases(path: Path) -> list[tuple[int, int]]:
    lines: list[tuple[int, int]] = []
    if not path.exists():
        return lines
    for n, raw in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        entry = raw.split("#")[0].strip()
        if not entry:
            continue
        m = re.fullmatch(r"(\d+)\.(\d+)", entry)
        if not m:
            raise Problem(f"releases.txt:{n}: `{entry}` is not `X.Y`")
        lines.append((int(m[1]), int(m[2])))
    return lines


def merge(migrations: Path, version: str) -> list[str]:
    """Does the merge; returns what it did, one line each."""
    m = re.fullmatch(r"(\d+)\.(\d+)\.(\d+)", version)
    if not m:
        raise Problem(f"`{version}` is not a version X.Y.Z")
    major, minor, patch = int(m[1]), int(m[2]), int(m[3])
    previews = sorted((migrations / "preview").glob("*.sql")) if (migrations / "preview").is_dir() else []
    if patch != 0:
        if previews:
            names = ", ".join(p.name for p in previews)
            raise Problem(f"{version} is a patch release, but there are preview updates ({names}): "
                          f"the next release must be a minor release (R-OPS-018)")
        return [f"{version} is a patch release without database updates: nothing to do"]
    releases_file = migrations / "releases.txt"
    releases = read_releases(releases_file)
    line = (major, minor)
    if line in releases:
        raise Problem(f"{major}.{minor} is already released (releases.txt)")
    if releases and line < releases[-1]:
        raise Problem(f"{major}.{minor} is older than the last release {releases[-1][0]}.{releases[-1][1]}")
    for p in previews:
        if not PREVIEW_NAME.match(p.stem):
            raise Problem(f"preview/{p.name}: a preview is named `NNNN-topic.sql`")
    target = migrations / f"{version}.sql"
    parsed = [(p, parse(p)) for p in previews]
    for p, f in parsed:
        if f["previews"]:
            raise Problem(f"preview/{p.name}: only release updates list previews")
    done: list[str] = []
    if parsed:
        if target.exists():
            base = parse(target)
            done.append(f"appended to the unreleased {target.name}")
        else:
            base = {
                "header": [f"-- Musicus {version}: database update, merged from the previews listed below by\n",
                           "-- scripts/release-migrations.py. Released updates are never edited.\n"],
                "previews": [],
                "sections": ["", "", ""],
            }
            done.append(f"wrote {target.name}")
        listed = base["previews"] + [p.stem for p, _ in parsed]
        if len(set(listed)) != len(listed):
            raise Problem(f"{target.name} already lists one of the previews")
        sections = list(base["sections"])
        for p, f in parsed:
            for i, body in enumerate(f["sections"]):
                if i == 0 or not is_empty(body):
                    intro = f"-- ---- preview {p.stem} ----\n" + ("".join(f["header"]) if i == 0 else "")
                    sep = "" if not sections[i] or sections[i].endswith("\n\n") else "\n"
                    sections[i] += sep + intro + body
        text = "".join(base["header"]) + f"{PREVIEWS} {' '.join(listed)}\n"
        for marker, body in zip(MARKERS, sections):
            if marker == MARKERS[0] or not is_empty(body):
                text += marker + "\n" + body
        target.write_text(text, encoding="utf-8")
        for p, _ in parsed:
            p.unlink()
            done.append(f"merged and deleted preview/{p.name}")
        try:
            (migrations / "preview").rmdir()
        except OSError:
            pass
    else:
        done.append("no preview updates: no database update in this release")
    text = releases_file.read_text(encoding="utf-8") if releases_file.exists() else ""
    if text and not text.endswith("\n"):
        text += "\n"
    releases_file.write_text(text + f"{major}.{minor}\n", encoding="utf-8")
    done.append(f"added {major}.{minor} to releases.txt")
    return done


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("version", help="the release version X.Y.Z")
    ap.add_argument("--migrations", type=Path, default=DEFAULT_DIR, help="the migrations folder")
    args = ap.parse_args()
    try:
        for line in merge(args.migrations, args.version):
            print(line)
    except Problem as e:
        print(f"release-migrations: {e}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
