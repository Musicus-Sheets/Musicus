#!/usr/bin/env python3
"""Tests of the release pipeline's scripts: release-check.py (on temporary git repositories, with
the real merge script), release-notes.py and package-program.py. Run by `cargo test`
(crates/musicus-server/tests/release_tools.rs::release_tools_unit_tests) or directly."""

from __future__ import annotations

import hashlib
import importlib.util
import os
import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest
import zipfile
from pathlib import Path

HERE = Path(__file__).resolve().parent


def load(name: str, file: str):
    spec = importlib.util.spec_from_file_location(name, HERE / file)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


rc = load("release_check", "release-check.py")
notes = load("release_notes", "release-notes.py")
pkg = load("package_program", "package-program.py")

M = "crates/musicus-server/migrations/"
P = M + "preview/"
RELEASES = "# Released lines, oldest first.\n"
PREVIEW = "-- a preview\n-- @additive\nCREATE TABLE {name} (id INTEGER PRIMARY KEY);\n"


def cargo(version: str) -> dict[str, str]:
    toml = f'[workspace]\nmembers = []\n\n[workspace.package]\nversion = "{version}"\nedition = "2024"\n'
    lock = "version = 4\n\n" + "".join(
        f'[[package]]\nname = "{n}"\nversion = "{version}"\n\n' for n in ("musicus-api", "musicus-server"))
    lock += '[[package]]\nname = "serde"\nversion = "1.0.0"\nsource = "registry+https://example"\n'
    return {"Cargo.toml": toml, "Cargo.lock": lock}


class Repo:
    """`main` at version 0.1.0 with two previews; `release/<v>` branches are made from it."""

    def __init__(self, root: Path, releases: str = RELEASES, previews: tuple[str, ...] = ("0001-a", "0002-b"),
                 version: str = "0.1.0"):
        self.root = root
        self.git("init", "-q", "-b", "main")
        files = {M + "releases.txt": releases, "README.md": "x\n", **cargo(version)}
        files.update({P + f"{p}.sql": PREVIEW.format(name=p.replace("-", "_")) for p in previews})
        self.write(files)
        self.commit("base")

    def git(self, *args: str) -> str:
        env = {**os.environ, "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@t", "GIT_COMMITTER_NAME": "t",
               "GIT_COMMITTER_EMAIL": "t@t", "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1"}
        return subprocess.run(["git", *args], cwd=self.root, check=True, capture_output=True, text=True,
                              env=env).stdout

    def write(self, files: dict[str, str | None]) -> None:
        for name, text in files.items():
            path = self.root / name
            if text is None:
                path.unlink()
                continue
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text)

    def commit(self, msg: str) -> None:
        self.git("add", "-A")
        self.git("commit", "-q", "--allow-empty", "-m", msg)

    def release(self, version: str, merge: bool = True) -> None:
        """Branch release/<version>, raise the version, run the merge script (as RELEASING.md § 2)."""
        self.git("switch", "-q", "-c", f"release/{version}", "main")
        self.write(cargo(version))
        if merge:
            out = subprocess.run([sys.executable, str(HERE / "release-migrations.py"), "--migrations",
                                  str(self.root / M), "--no-simplify", version], capture_output=True, text=True)
            assert out.returncode == 0, out.stderr
        self.commit(f"release {version}")

    def check(self, branch: str | None = None) -> tuple[list[tuple[str, str]], str]:
        branch = branch or self.git("branch", "--show-current").strip()
        return rc.check("main", "HEAD", branch, cwd=str(self.root))


class ReleaseCheck(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def assertProblem(self, found, path: str, text: str):
        self.assertTrue(any(p == path and text in m for p, m in found), f"no `{text}` for {path}: {found}")

    def test_minor_with_merged_previews_passes(self):
        repo = Repo(self.root)
        repo.release("1.0.0")
        found, summary = repo.check()
        self.assertEqual(found, [])
        self.assertIn("2 preview(s) merged into " + M + "1.0.0.sql", summary)
        self.assertIn("line 1.0 added", summary)

    def test_script_exit_codes(self):
        repo = Repo(self.root)
        repo.release("1.0.0")
        env = {k: v for k, v in os.environ.items() if k != "GITHUB_ACTIONS"}
        ok = subprocess.run([sys.executable, str(HERE / "release-check.py"), "--branch", "release/1.0.0", "main"],
                            cwd=self.root, capture_output=True, text=True, env=env)
        self.assertEqual(ok.returncode, 0, ok.stderr)
        self.assertIn("release check passed: 1.0.0: major release", ok.stdout)
        env["GITHUB_ACTIONS"] = "true"
        bad = subprocess.run([sys.executable, str(HERE / "release-check.py"), "--branch", "release/1.0.1", "main"],
                             cwd=self.root, capture_output=True, text=True, env=env)
        self.assertEqual(bad.returncode, 1)
        self.assertIn("::error file=Cargo.toml::", bad.stdout)

    def test_branch_name(self):
        repo = Repo(self.root)
        repo.release("1.0.0")
        for branch in ("feature/x", "release/1.0", "release/v1.0.0", "release/01.0.0"):
            found, _ = repo.check(branch)
            self.assertProblem(found, "", "is not a release branch")

    def test_version_must_match_branch_and_lock(self):
        repo = Repo(self.root)
        repo.release("1.0.0")
        found, _ = repo.check("release/1.1.0")
        self.assertProblem(found, "Cargo.toml", "the workspace version is 1.0.0, the release branch says 1.1.0")
        self.assertProblem(found, "Cargo.lock", "musicus-api has version 1.0.0")
        repo.write({"Cargo.lock": cargo("0.1.0")["Cargo.lock"]})
        repo.commit("lock not updated")
        found, _ = repo.check()
        self.assertEqual({p for p, _ in found}, {"Cargo.lock"}, found)

    def test_version_must_be_newer_than_main(self):
        repo = Repo(self.root, releases=RELEASES + "1.0\n", previews=(), version="1.1.0")
        repo.release("1.0.1")
        found, _ = repo.check()
        self.assertProblem(found, "Cargo.toml", "1.0.1 is not newer than the version on the base branch (1.1.0)")

    def test_previews_left(self):
        repo = Repo(self.root)
        repo.release("1.0.0")
        repo.write({P + "0003-late.sql": PREVIEW.format(name="late")})
        repo.commit("a preview after the merge")
        found, _ = repo.check()
        self.assertProblem(found, P + "0003-late.sql", "preview update left")
        self.assertProblem(found, P + "0003-late.sql", "preview added or changed in the release")

    def test_not_merged(self):
        repo = Repo(self.root)
        repo.release("1.0.0", merge=False)
        found, _ = repo.check()
        self.assertProblem(found, M + "1.0.0.sql", "missing")
        self.assertProblem(found, M + "releases.txt", "must gain exactly the line `1.0`")
        self.assertProblem(found, P + "0001-a.sql", "preview update left")

    def test_listed_previews_must_match_the_removed_ones(self):
        repo = Repo(self.root)
        repo.release("1.0.0")
        path = repo.root / M / "1.0.0.sql"
        path.write_text(path.read_text().replace("-- @previews: 0001-a 0002-b", "-- @previews: 0001-a"))
        repo.commit("edit the header")
        found, _ = repo.check()
        self.assertProblem(found, M + "1.0.0.sql", "lists 0001-a, but the pull request removes 0001-a 0002-b")
        path.write_text(path.read_text().replace("-- @additive", "-- @unknown", 1))
        repo.commit("break it")
        found, _ = repo.check()
        self.assertProblem(found, M + "1.0.0.sql", "not a valid update file")

    def test_releases_txt_gains_exactly_the_line(self):
        repo = Repo(self.root)
        repo.release("1.0.0")
        repo.write({M + "releases.txt": RELEASES + "0.9\n1.0\n"})
        repo.commit("two lines")
        found, _ = repo.check()
        self.assertProblem(found, M + "releases.txt", "must gain exactly the line `1.0`")

    def test_other_migration_changes_fail(self):
        repo = Repo(self.root, releases=RELEASES + "1.0\n", previews=("0003-c",), version="1.0.0")
        repo.write({M + "1.0.0.sql": "-- @previews: 0001-a\n-- @additive\nCREATE TABLE a (id INTEGER);\n"})
        repo.commit("released 1.0.0 on main")
        repo.release("1.1.0")
        repo.write({M + "1.0.0.sql": "-- @previews: 0001-a\n-- @additive\nCREATE TABLE a (id INTEGER, x TEXT);\n"})
        repo.commit("edit a released update")
        found, _ = repo.check()
        self.assertEqual([p for p, _ in found], [M + "1.0.0.sql"], found)
        self.assertProblem(found, M + "1.0.0.sql", "changed in the release (M)")

    def test_minor_without_previews(self):
        repo = Repo(self.root, releases=RELEASES + "1.0\n", previews=(), version="1.0.0")
        repo.release("1.1.0")
        found, summary = repo.check()
        self.assertEqual(found, [])
        self.assertIn("no database update", summary)
        repo.write({M + "1.1.0.sql": "-- @additive\n"})
        repo.commit("an update without previews")
        found, _ = repo.check()
        self.assertProblem(found, M + "1.1.0.sql", "has no preview updates")

    def test_line_already_released_or_older(self):
        repo = Repo(self.root, releases=RELEASES + "1.0\n2.0\n", previews=(), version="1.0.0")
        repo.git("switch", "-q", "-c", "release/1.5.0")
        repo.write({**cargo("1.5.0"), M + "releases.txt": RELEASES + "1.0\n2.0\n1.5\n"})
        repo.commit("older line")
        found, _ = repo.check()
        self.assertProblem(found, M + "releases.txt", "1.5 is older than the last released line 2.0")

    def test_patch(self):
        repo = Repo(self.root, releases=RELEASES + "1.0\n", previews=(), version="1.0.0")
        repo.release("1.0.1")
        found, summary = repo.check()
        self.assertEqual(found, [])
        self.assertIn("patch release, no database update", summary)
        repo.write({M + "releases.txt": RELEASES + "1.0\n1.0\n"})
        repo.commit("touch releases.txt")
        found, _ = repo.check()
        self.assertProblem(found, M + "releases.txt", "changed in the patch release 1.0.1 (M)")

    def test_patch_with_a_preview_fails(self):
        repo = Repo(self.root, releases=RELEASES + "1.0\n", previews=("0001-a",), version="1.0.0")
        repo.release("1.0.1", merge=False)
        found, _ = repo.check()
        self.assertProblem(found, P + "0001-a.sql", "a patch contains no database update (R-OPS-018)")
        repo.write({M + "1.0.1.sql": "-- @additive\n"})
        repo.commit("a patch update")
        found, _ = repo.check()
        self.assertProblem(found, M + "1.0.1.sql", "a patch release has no database update of its own")

    def test_patch_of_an_older_line_fails(self):
        repo = Repo(self.root, releases=RELEASES + "1.0\n1.1\n", previews=(), version="1.0.0")
        repo.release("1.0.2")
        found, _ = repo.check()
        self.assertProblem(found, M + "releases.txt", "the last released line is 1.1")


class TitlePages(unittest.TestCase):
    def test_html(self):
        with tempfile.TemporaryDirectory() as tmp:
            good = Path(tmp) / "good.html"
            good.write_text('<header><p class="version">User guide for Musicus 1.2.0</p></header>Musicus 1.0.0')
            dev = Path(tmp) / "dev.html"
            dev.write_text('<p class="version">User guide for a development version of Musicus</p>')
            other = Path(tmp) / "other.html"
            other.write_text('<p class="version">Describes Musicus 1.2.0.1</p><p>Musicus 1.2.0</p>')
            self.assertEqual(rc.check_title_pages("1.2.0", [good]), [])
            self.assertEqual(len(rc.check_title_pages("1.2.0", [dev, other])), 2)
            self.assertEqual(len(rc.check_title_pages("1.0.0", [good])), 1)

    def test_missing_file(self):
        found = rc.check_title_pages("1.0.0", [Path("/nonexistent/x.html")])
        self.assertIn("cannot read the title page", found[0][1])


class ReleaseNotes(unittest.TestCase):
    BODY = ("Release 1.2.0.\r\n\r\n## Checklist\r\n- [x] done\r\n\r\n## Release notes\r\n<!-- write here -->\r\n"
            "New: programmes.\r\n\r\n### Fixes\r\n- one\r\n```\r\n## not a heading\r\n```\r\n## Other\r\nnot this\r\n")

    def test_section(self):
        text = notes.section(self.BODY)
        self.assertTrue(text.startswith("New: programmes."), text)
        self.assertIn("### Fixes", text)
        self.assertIn("## not a heading", text)
        self.assertNotIn("not this", text)
        self.assertNotIn("write here", text)

    def test_missing_or_empty(self):
        with self.assertRaises(notes.Missing):
            notes.section("## Checklist\n- x\n")
        with self.assertRaises(notes.Missing):
            notes.section("## Release notes\n<!-- todo -->\n\n## Next\n")

    def test_notes(self):
        text = notes.notes(self.BODY, "1.2.0", ["ghcr.io/o/m:1.2.0", "ghcr.io/o/m:1.2"], ["macos-x86_64"])
        self.assertIn("macOS (Intel): the program could not be built", text)
        self.assertIn("- `ghcr.io/o/m:1.2`", text)
        self.assertIn("sha256sum -c SHA256SUMS", text)
        self.assertNotIn("Not available", notes.notes(self.BODY, "1.2.0", [], []))


class PackageProgram(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = Path(self.tmp.name)
        self.program = self.dir / "musicus-server"
        self.program.write_bytes(b"\x7fELF program")
        os.environ["SOURCE_DATE_EPOCH"] = "1767225600"

    def tearDown(self):
        os.environ.pop("SOURCE_DATE_EPOCH", None)
        self.tmp.cleanup()

    def test_tar_and_sums(self):
        out = self.dir / "dist"
        a = pkg.archive(self.program, "1.2.0", "linux-arm64", out, "https://example.org/guide/")
        self.assertEqual(a.name, "musicus-1.2.0-linux-arm64.tar.gz")
        with tarfile.open(a) as t:
            names = t.getnames()
            exe = t.getmember("musicus-1.2.0-linux-arm64/musicus-server")
            self.assertEqual(exe.mode, 0o755)
            self.assertEqual(exe.mtime, 1767225600)
            self.assertEqual(t.extractfile(exe).read(), b"\x7fELF program")
            readme = t.extractfile("musicus-1.2.0-linux-arm64/README.txt").read().decode()
        self.assertIn("musicus-1.2.0-linux-arm64/LICENSE", names)
        self.assertIn("./musicus-server serve", readme)
        self.assertIn("https://example.org/guide/", readme)
        again = pkg.archive(self.program, "1.2.0", "linux-arm64", self.dir / "again", "https://example.org/guide/")
        self.assertEqual(a.read_bytes(), again.read_bytes(), "the same program gives the same archive")

        z = pkg.archive(self.program, "1.2.0", "windows-x86_64", out, None)
        self.assertEqual(z.name, "musicus-1.2.0-windows-x86_64.zip")
        with zipfile.ZipFile(z) as f:
            self.assertEqual(f.read("musicus-1.2.0-windows-x86_64/musicus-server.exe"), b"\x7fELF program")
            readme = f.read("musicus-1.2.0-windows-x86_64/README.txt").decode()
        self.assertIn("\r\nmusicus-server.exe serve", readme.replace("    ", ""))
        self.assertNotIn("User guide", readme)

        (out / "notes.md").write_text("not an archive")
        sums = pkg.sums(out).read_text()
        lines = sums.splitlines()
        self.assertEqual([line.split("  ")[1] for line in lines],
                         ["musicus-1.2.0-linux-arm64.tar.gz", "musicus-1.2.0-windows-x86_64.zip"])
        self.assertEqual(lines[0].split("  ")[0], hashlib.sha256(a.read_bytes()).hexdigest())
        if shutil.which("sha256sum"):
            check = subprocess.run(["sha256sum", "-c", "SHA256SUMS"], cwd=out, capture_output=True, text=True)
            self.assertEqual(check.returncode, 0, check.stdout + check.stderr)

    def test_bad_input(self):
        with self.assertRaises(SystemExit):
            pkg.archive(self.program, "1.2", "linux-x86_64", self.dir, None)
        with self.assertRaises(SystemExit):
            pkg.archive(self.program, "1.2.0", "linux-riscv", self.dir, None)
        with self.assertRaises(SystemExit):
            pkg.sums(self.dir)


class SmokeHelpers(unittest.TestCase):
    def test_pdf_and_ink(self):
        smoke = load("smoke_program", "smoke-program.py")
        self.assertTrue(smoke.tiny_pdf(2).startswith(b"%PDF-1.7\n"))
        ink = smoke.ink_page()
        self.assertEqual(ink[:3], bytes([1, 1, 0]))  # version 1, one object, a stroke
        self.assertEqual(smoke.varint(300), bytes([0xAC, 0x02]))
        self.assertEqual([smoke.zigzag(n) for n in (0, -1, 1, -2)], [0, 1, 2, 3])


if __name__ == "__main__":
    unittest.main()
