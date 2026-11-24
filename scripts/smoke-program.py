#!/usr/bin/env python3
"""Smoke test of a built Musicus program (R-OPS-032, R-OPS-029; a-release-pipeline).

    scripts/smoke-program.py --program target/release/musicus-server [--version X.Y.Z]
                             [--web builtin|dir|none] [--stop ctrl-c|kill] [-- SERVER ARGS...]

Starts the program on a new, empty data directory and a free port on 127.0.0.1 and checks, over
HTTP like a client: the web client is served (`/` and the files `index.html` names) the way
`--web` says (default `builtin`: built into the program, R-OPS-030); `/api/version` names
`--version`; the admin's invite link from the log sets a password; a piece with a part, an
uploaded PDF and a saved marking page (read back unchanged); `backup` while the server runs
gives a database holding that page; Ctrl+C (`--stop ctrl-c`, the default) ends the server
within 20 s with exit code 0 and "shutting down" in the log; a second start on the same data
directory serves the part again. Arguments after `--` are passed to `serve` (e.g. `--web-dir`).

Ctrl+C on Windows: the program is started by a helper in a console of its own (hidden), which
sends CTRL_C_EVENT to that console only. `--stop kill` ends the program without Ctrl+C.

Only the Python standard library; the program's log is printed when a check fails.
Exit code 0 if everything passed, 1 otherwise. Used by .github/workflows/release.yml and by
crates/musicus-server/tests/release_tools.rs (debug build, client from the test fixture).
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import signal
import socket
import sqlite3
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
from pathlib import Path

WINDOWS = os.name == "nt"
PASSWORD = "smoke-test-password"
OPENER = urllib.request.build_opener(urllib.request.ProxyHandler({}))  # never through a proxy


class Failed(Exception):
    pass


def tiny_pdf(pages: int = 1) -> bytes:
    """A minimal valid PDF with empty A4 pages (as `processors::tests::tiny_pdf`)."""
    objects = ["<< /Type /Catalog /Pages 2 0 R >>",
               f"<< /Type /Pages /Kids [{' '.join(f'{i + 3} 0 R' for i in range(pages))}] /Count {pages} >>"]
    objects += ["<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] >>"] * pages
    pdf = bytearray(b"%PDF-1.7\n")
    offsets = []
    for i, obj in enumerate(objects):
        offsets.append(len(pdf))
        pdf += f"{i + 1} 0 obj\n{obj}\nendobj\n".encode()
    xref = len(pdf)
    pdf += f"xref\n0 {len(objects) + 1}\n0000000000 65535 f \n".encode()
    pdf += "".join(f"{off:010} 00000 n \n" for off in offsets).encode()
    pdf += f"trailer\n<< /Size {len(objects) + 1} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n".encode()
    return bytes(pdf)


def varint(n: int) -> bytes:
    out = bytearray()
    while True:
        byte = n & 0x7F
        n >>= 7
        if n:
            out.append(byte | 0x80)
        else:
            out.append(byte)
            return bytes(out)


def zigzag(n: int) -> int:
    return (n << 1) ^ (n >> 63)


def ink_page() -> bytes:
    """One pen stroke in the binary ink format: version byte 1 + postcard(PageLayer)
    (musicus-api `ink.rs`: objects[Stroke{id, tool Pen, color, width, start, deltas, pressure}])."""
    points = [(1000, 2000, 128), (1500, 2100, 140), (2200, 2300, 150)]
    deltas = [(b[0] - a[0], b[1] - a[1]) for a, b in zip(points, points[1:])]
    body = varint(1) + varint(0)  # one object, variant Stroke
    body += varint(0x5EED_0001) + varint(0) + varint(0x1D3557FF) + varint(120)  # id, Pen, colour, width
    body += varint(points[0][0]) + varint(points[0][1])
    body += varint(len(deltas)) + b"".join(varint(zigzag(dx)) + varint(zigzag(dy)) for dx, dy in deltas)
    body += varint(len(points)) + bytes(p[2] for p in points)
    return bytes([1]) + body


def free_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


class Response:
    def __init__(self, status: int, headers, body: bytes):
        self.status, self.headers, self.body = status, headers, body

    def json(self):
        return json.loads(self.body)


def request(method: str, url: str, body: bytes | dict | None = None, headers: dict | None = None,
            timeout: float = 30) -> Response:
    headers = dict(headers or {})
    if isinstance(body, dict):
        body = json.dumps(body).encode()
        headers.setdefault("Content-Type", "application/json")
    req = urllib.request.Request(url, data=body, method=method, headers=headers)
    try:
        with OPENER.open(req, timeout=timeout) as r:
            return Response(r.status, r.headers, r.read())
    except urllib.error.HTTPError as e:
        return Response(e.code, e.headers, e.read())


class Server:
    """The program running `serve` on `data`, its output in `log`."""

    def __init__(self, program: Path, data: Path, work: Path, extra: list[str], stop: str, n: int):
        self.port = free_port()
        self.base = f"http://127.0.0.1:{self.port}"
        self.log = work / f"server-{n}.log"
        self.stop_mode = stop
        args = [str(program), "serve", "--data-dir", str(data), "--listen", f"127.0.0.1:{self.port}",
                "--public-url", self.base, *extra]
        env = {**os.environ, "NO_COLOR": "1", "RUST_LOG": "info"}
        env.pop("MUSICUS_WEB_DIR", None)
        env.pop("MUSICUS_DATA_DIR", None)
        self.helper = None
        if WINDOWS and stop == "ctrl-c":
            # A console of its own: CTRL_C_EVENT reaches every process of a console, so it must not
            # be the console of this script (or of the CI step).
            self.ctl = work / f"server-{n}"
            self.ctl.mkdir()
            (self.ctl / "args.json").write_text(json.dumps(args))
            info = subprocess.STARTUPINFO()
            info.dwFlags |= subprocess.STARTF_USESHOWWINDOW
            info.wShowWindow = 0  # SW_HIDE
            self.helper = subprocess.Popen(
                [sys.executable, os.path.abspath(__file__), "--console-host", str(self.ctl), str(self.log)],
                env=env, creationflags=subprocess.CREATE_NEW_CONSOLE, startupinfo=info)
            self.proc = None
        else:
            self.out = open(self.log, "wb")
            self.proc = subprocess.Popen(args, stdout=self.out, stderr=subprocess.STDOUT, env=env,
                                         stdin=subprocess.DEVNULL)

    def text(self) -> str:
        try:
            return re.sub(r"\x1b\[[0-9;]*m", "", self.log.read_text(encoding="utf-8", errors="replace"))
        except OSError:
            return ""

    def exited(self) -> int | None:
        if self.helper is not None:
            done = self.ctl / "exit"
            if done.exists() and done.read_text().strip():
                return int(done.read_text())
            return -1 if self.helper.poll() is not None else None
        return self.proc.poll()

    def wait_ready(self, timeout: float = 120) -> None:
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            if (code := self.exited()) is not None:
                raise Failed(f"the program exited at start (exit code {code})")
            try:
                if request("GET", f"{self.base}/api/health", timeout=2).status == 200:
                    return
            except OSError:
                pass
            time.sleep(0.3)
        raise Failed(f"the program did not answer /api/health within {timeout:.0f} s")

    def stop(self) -> None:
        """Ctrl+C (or kill) and wait; fails unless the program ends cleanly."""
        if self.stop_mode == "kill":
            self.proc.kill()
            self.proc.wait(timeout=20)
            self.out.close()
            return
        if self.helper is not None:
            (self.ctl / "stop").write_text("")
            end = time.monotonic() + 30
            while (code := self.exited()) is None and time.monotonic() < end:
                time.sleep(0.2)
            if code is None:
                self.helper.kill()
                raise Failed("the program did not end within 20 s after Ctrl+C")
            self.helper.wait(timeout=10)
        else:
            self.proc.send_signal(signal.SIGINT)
            try:
                code = self.proc.wait(timeout=20)
            except subprocess.TimeoutExpired:
                self.proc.kill()
                self.proc.wait()
                raise Failed("the program did not end within 20 s after Ctrl+C") from None
            finally:
                self.out.close()
        if code != 0:
            raise Failed(f"the program ended with exit code {code} after Ctrl+C")
        if "shutting down" not in self.text():
            raise Failed("the log does not say 'shutting down' after Ctrl+C")

    def kill(self) -> None:
        for p in (self.proc, self.helper):
            if p is not None and p.poll() is None:
                p.kill()
                p.wait()
        if self.proc is not None:
            self.out.close()


def console_host(ctl: Path, log: Path) -> int:
    """Windows helper (own console): runs the program, Ctrl+C on request, writes its exit code."""
    signal.signal(signal.SIGINT, signal.SIG_IGN)  # the CTRL_C_EVENT below reaches this process too
    args = json.loads((ctl / "args.json").read_text())
    with open(log, "wb") as out:
        proc = subprocess.Popen(args, stdout=out, stderr=subprocess.STDOUT, stdin=subprocess.DEVNULL)
        while proc.poll() is None and not (ctl / "stop").exists():
            time.sleep(0.2)
        if proc.poll() is None:
            os.kill(0, signal.CTRL_C_EVENT)
            try:
                proc.wait(timeout=20)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait()
                (ctl / "exit").write_text("")
                return 1
    (ctl / "exit").write_text(str(proc.returncode))
    return 0


def check(condition: bool, message: str) -> None:
    if not condition:
        raise Failed(message)
    print(f"ok - {message}")


def smoke(args: argparse.Namespace, work: Path) -> None:
    program = args.program.resolve()
    out = subprocess.run([str(program), "--version"], capture_output=True, text=True, timeout=60)
    check(out.returncode == 0, f"`--version` runs: {out.stdout.strip()}")
    if args.version:
        check(out.stdout.strip() == f"musicus-server {args.version}", f"`--version` names {args.version}")

    data = work / "data"
    data.mkdir()
    server = Server(program, data, work, args.server_args, args.stop, 1)
    servers = [server]
    try:
        server.wait_ready()
        print(f"ok - started on {server.base}")
        log = server.text()
        expected = {"builtin": "web client: built into the program", "dir": "web client: folder",
                    "none": "contains no web client"}[args.web]
        check(expected in log, f"the log says '{expected}'")
        base = server.base

        version = request("GET", f"{base}/api/version")
        check(version.status == 200, "GET /api/version answers")
        if args.version:
            check(version.json().get("version") == args.version, f"/api/version names {args.version}")

        index = request("GET", f"{base}/")
        if args.web == "none":
            check(index.status == 404, "no web client: / is not served")
        else:
            check(index.status == 200 and b"<html" in index.body.lower(), "GET / serves the web client")
            refs = re.findall(rb'(?:src|href)="(/?[^":?#]+\.(?:js|wasm|css))"', index.body)
            for ref in sorted(set(refs))[:20]:
                url = f"{base}/{ref.decode().removeprefix('./').lstrip('/')}"
                check(request("GET", url).status == 200, f"GET {url[len(base):]} (named in index.html)")

        m = re.search(r"/invite/([0-9a-f]{16,})", log)
        check(m is not None, "the log shows the admin's invite link")
        accepted = request("POST", f"{base}/api/auth/invites/{m[1]}", {"password": PASSWORD})
        check(accepted.status == 200, "the invite sets the admin's password")
        login = request("POST", f"{base}/api/auth/login", {"username": "admin", "password": PASSWORD, "bearer": True})
        check(login.status == 200 and login.json().get("token"), "the admin logs in")
        auth = {"Authorization": f"Bearer {login.json()['token']}"}

        instrument = request("POST", f"{base}/api/instruments", {"name": "Trumpet"}, auth)
        check(instrument.status == 200, "an instrument is created")
        score = request("POST", f"{base}/api/scores", {"title": "Smoke test"}, auth)
        check(score.status == 200, "a piece is created")
        part = request("POST", f"{base}/api/scores/{score.json()['id']}/parts",
                       {"instrument_ids": [instrument.json()["id"]]}, auth)
        check(part.status == 200, "a part is created")
        part_id = part.json()["id"]
        upload = request("PUT", f"{base}/api/parts/{part_id}/file", tiny_pdf(2),
                         {**auth, "Content-Type": "application/pdf"})
        check(upload.status == 200, f"a PDF is uploaded (status {upload.status})")
        got = request("GET", f"{base}/api/parts/{part_id}", headers=auth)
        check(got.status == 200 and got.json().get("file"), "the part has the file")
        sha = got.json()["file"]["sha256"]
        pdf = request("GET", f"{base}/api/files/{sha}", headers=auth)
        check(pdf.status == 200 and pdf.body == tiny_pdf(2), "the file is served unchanged")

        sets = request("POST", f"{base}/api/parts/{part_id}/annotation-sets", {}, auth)
        check(sets.status == 200, "an annotation set is created")
        set_id = sets.json()["id"]
        ink = ink_page()
        saved = request("PUT", f"{base}/api/annotation-sets/{set_id}/pages/0", ink,
                        {**auth, "If-Match": '"0"', "Content-Type": "application/octet-stream"})
        check(saved.status == 200 and saved.json().get("revision") == 1,
              f"a marking is saved (status {saved.status}: {saved.body[:200]!r})")
        page = request("GET", f"{base}/api/annotation-sets/{set_id}/pages/0", headers=auth)
        check(page.status == 200 and page.body == ink, "the marking is read back unchanged")

        backup = work / "backup.sqlite"
        out = subprocess.run([str(program), "backup", "--data-dir", str(data), "--to", str(backup)],
                             capture_output=True, text=True, timeout=120,
                             env={**os.environ, "NO_COLOR": "1"})
        said = next((line for line in out.stdout.splitlines() if "written to" in line), out.stdout.strip()[:200])
        check(out.returncode == 0, f"`backup` runs while the server runs: {said}"
              + (f" {out.stderr.strip()}" if out.returncode else ""))
        with sqlite3.connect(f"file:{backup.as_posix()}?mode=ro", uri=True) as db:
            pages = db.execute("SELECT COUNT(*) FROM annotation_pages").fetchone()[0]
        check(pages == 1, "the backup holds the saved marking")

        server.stop()
        print(f"ok - stopped ({'Ctrl+C' if args.stop == 'ctrl-c' else 'killed'})")

        server = Server(program, data, work, args.server_args, args.stop, 2)
        servers.append(server)
        server.wait_ready()
        again = request("GET", f"{server.base}/api/parts/{part_id}", headers=auth)
        check(again.status == 200 and again.json().get("file"), "a second start on the same data serves the part")
        server.stop()
        print("ok - stopped again")
    except BaseException:
        for s in servers:
            s.kill()
            print(f"--- log of {s.log.name} ---\n{s.text()}--- end of log ---", file=sys.stderr)
        raise


def main(argv: list[str]) -> int:
    if argv[:1] == ["--console-host"]:
        return console_host(Path(argv[1]), Path(argv[2]))
    server_args: list[str] = []
    if "--" in argv:
        i = argv.index("--")
        argv, server_args = argv[:i], argv[i + 1:]
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--program", type=Path, required=True, help="the built musicus-server program")
    ap.add_argument("--version", help="the version the program must report")
    ap.add_argument("--web", choices=["builtin", "dir", "none"], default="builtin",
                    help="how the web client must be served (default: built into the program)")
    ap.add_argument("--stop", choices=["ctrl-c", "kill"], default="ctrl-c", help="how to end the server")
    ap.add_argument("--keep", action="store_true", help="keep the data directory and logs")
    args = ap.parse_args(argv)
    args.server_args = server_args
    work = Path(tempfile.mkdtemp(prefix="musicus-smoke-"))
    try:
        smoke(args, work)
    except (Failed, OSError, subprocess.SubprocessError, ValueError, KeyError, sqlite3.Error) as e:
        print(f"smoke test FAILED: {e}", file=sys.stderr)
        return 1
    finally:
        if args.keep:
            print(f"kept {work}")
        else:
            shutil.rmtree(work, ignore_errors=True)
    print("smoke test passed")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
