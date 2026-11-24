#!/usr/bin/env python3
"""Program archives of a release and their checksums (R-OPS-029; a-release-pipeline).

    scripts/package-program.py archive --program PATH --version X.Y.Z --platform PLATFORM --out DIR
                                       [--guide-url URL]
    scripts/package-program.py sums DIR

`archive` writes DIR/musicus-X.Y.Z-PLATFORM.tar.gz (`.zip` for windows-*), holding one folder
musicus-X.Y.Z-PLATFORM/ with the program (`musicus-server`, `musicus-server.exe` on Windows;
executable), LICENSE and README.txt (how to start it; the user guide's address with --guide-url).
File times are SOURCE_DATE_EPOCH when set (the release commit's time), so the same program gives
the same archive. PLATFORM is one of PLATFORMS.

`sums` writes DIR/SHA256SUMS for every archive in DIR (sorted by name), in the format of
`sha256sum`, so `sha256sum -c SHA256SUMS` checks the downloads.

Used by .github/workflows/release.yml; tested by scripts/test_release_tools.py.
"""

from __future__ import annotations

import argparse
import gzip
import hashlib
import io
import os
import re
import sys
import tarfile
import time
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PLATFORMS = ["linux-x86_64", "linux-arm64", "windows-x86_64", "macos-arm64", "macos-x86_64"]
ARCHIVE = re.compile(r"^musicus-\d+\.\d+\.\d+-[a-z0-9_-]+\.(tar\.gz|zip)$")

README = """Musicus {version} for {platform}
{rule}

Musicus is a sheet-music library for ensembles: members read the parts of their group
and draw their own markings. This folder holds the whole program, web interface included.

Start it (stop it with Ctrl+C):

    {start} serve --public-url http://<name or address of this computer>:8080

At the first start it prints a link to set the password of the user "admin". It keeps its
data in the usual data folder of this system (see `{start} serve --help`, option --data-dir).
{guide}
Musicus is free software under the GNU Affero General Public License, version 3 (LICENSE).
Source code: https://github.com/Musicus-Sheets/Musicus
"""


def epoch() -> int:
    value = os.environ.get("SOURCE_DATE_EPOCH", "")
    return int(value) if value.isdigit() else int(time.time())


def archive(program: Path, version: str, platform: str, out: Path, guide_url: str | None) -> Path:
    if platform not in PLATFORMS:
        raise SystemExit(f"package-program: unknown platform {platform} (one of {', '.join(PLATFORMS)})")
    if not re.fullmatch(r"\d+\.\d+\.\d+", version):
        raise SystemExit(f"package-program: `{version}` is not a version X.Y.Z")
    windows = platform.startswith("windows-")
    exe = "musicus-server.exe" if windows else "musicus-server"
    folder = f"musicus-{version}-{platform}"
    start = exe if windows else f"./{exe}"
    guide = f"\nUser guide (installation, first steps, updates): {guide_url}\n" if guide_url else ""
    title = f"Musicus {version} for {platform}"
    readme = README.format(version=version, platform=platform, rule="=" * len(title), start=start, guide=guide)
    if windows:
        readme = readme.replace("\n", "\r\n")
    files = [
        (f"{folder}/{exe}", program.read_bytes(), 0o755),
        (f"{folder}/LICENSE", (ROOT / "LICENSE").read_bytes(), 0o644),
        (f"{folder}/README.txt", readme.encode(), 0o644),
    ]
    out.mkdir(parents=True, exist_ok=True)
    mtime = epoch()
    if windows:
        target = out / f"{folder}.zip"
        stamp = time.gmtime(max(mtime, 315532800))[:6]  # zip times start in 1980
        with zipfile.ZipFile(target, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as z:
            for name, data, mode in files:
                info = zipfile.ZipInfo(name, stamp)
                info.external_attr = (0o100000 | mode) << 16
                info.compress_type = zipfile.ZIP_DEFLATED
                z.writestr(info, data)
    else:
        target = out / f"{folder}.tar.gz"
        buf = io.BytesIO()
        with tarfile.open(fileobj=buf, mode="w", format=tarfile.PAX_FORMAT) as t:
            for name, data, mode in [(folder, b"", 0o755)] + files:
                info = tarfile.TarInfo(name)
                info.mtime, info.uid, info.gid, info.uname, info.gname = mtime, 0, 0, "", ""
                if name == folder:
                    info.type, info.mode = tarfile.DIRTYPE, mode
                    t.addfile(info)
                else:
                    info.size, info.mode = len(data), mode
                    t.addfile(info, io.BytesIO(data))
        with open(target, "wb") as f, gzip.GzipFile(fileobj=f, mode="wb", mtime=mtime, filename="") as gz:
            gz.write(buf.getvalue())
    return target


def sums(folder: Path) -> Path:
    names = sorted(p.name for p in folder.iterdir() if p.is_file() and ARCHIVE.match(p.name))
    if not names:
        raise SystemExit(f"package-program: no program archives in {folder}")
    lines = [f"{hashlib.sha256((folder / n).read_bytes()).hexdigest()}  {n}\n" for n in names]
    target = folder / "SHA256SUMS"
    target.write_text("".join(lines), encoding="utf-8", newline="\n")
    return target


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="command", required=True)
    a = sub.add_parser("archive", help="write the archive of one platform")
    a.add_argument("--program", type=Path, required=True)
    a.add_argument("--version", required=True)
    a.add_argument("--platform", required=True, choices=PLATFORMS)
    a.add_argument("--out", type=Path, required=True)
    a.add_argument("--guide-url")
    s = sub.add_parser("sums", help="write SHA256SUMS for the archives in a folder")
    s.add_argument("folder", type=Path)
    args = ap.parse_args(argv)
    if args.command == "archive":
        print(archive(args.program, args.version, args.platform, args.out, args.guide_url))
    else:
        target = sums(args.folder)
        print(target.read_text(encoding="utf-8"), end="")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
