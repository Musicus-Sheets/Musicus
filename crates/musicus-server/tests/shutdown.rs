//! The real process on the stop signal (R-OPS-043, a-client-outage): an
//! upload that runs across the signal is stored, new work after the grace is
//! refused with `server_shutting_down`, the process exits with success after
//! the last request, and the database is closed cleanly. Also: a start that
//! fails still ends the process with an error (R-OPS-045).
#![cfg(unix)]

use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{SocketAddr, TcpStream},
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

struct Process {
    child: Child,
    addr: SocketAddr,
    invite: String,
    _lines: mpsc::Receiver<String>,
}

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

fn wait_line(lines: &mpsc::Receiver<String>, what: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let line = lines.recv_timeout(left).unwrap_or_else(|_| panic!("no log line with {what:?}"));
        if line.contains(what) {
            return line;
        }
    }
}

fn spawn(data: &std::path::Path, grace: u64) -> Process {
    let mut child = Command::new(env!("CARGO_BIN_EXE_musicus-server"))
        .args(["serve", "--listen", "127.0.0.1:0", "--insecure-cookies", "--shutdown-grace-secs"])
        .arg(grace.to_string())
        .arg("--data-dir")
        .arg(data)
        .env("RUST_LOG", "info")
        .env("NO_COLOR", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let out = child.stdout.take().unwrap();
    let (tx, lines) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            // Strip ANSI colours, keep the text.
            let plain: String = strip_ansi(&line);
            let _ = tx.send(plain);
        }
    });
    let listening = wait_line(&lines, "listening on http://");
    let addr = listening.split("http://").nth(1).unwrap().split_whitespace().next().unwrap().parse().unwrap();
    let invite_line = wait_line(&lines, "/invite/");
    let invite = invite_line.rsplit("/invite/").next().unwrap().trim().to_owned();
    wait_line(&lines, "ready");
    Process { child, addr, invite, _lines: lines }
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            for d in chars.by_ref() {
                if d.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// One HTTP/1.1 request on its own connection; returns status line + headers and body.
fn request(addr: SocketAddr, method: &str, path: &str, token: Option<&str>, body: &[u8]) -> (String, Vec<u8>) {
    let mut s = TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    s.write_all(head(method, path, token, body.len()).as_bytes()).unwrap();
    s.write_all(body).unwrap();
    read_response(&mut s)
}

fn head(method: &str, path: &str, token: Option<&str>, len: usize) -> String {
    let auth = token.map(|t| format!("Authorization: Bearer {t}\r\n")).unwrap_or_default();
    format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n{auth}\
         Content-Type: application/json\r\nContent-Length: {len}\r\n\r\n"
    )
}

fn read_response(s: &mut TcpStream) -> (String, Vec<u8>) {
    let mut all = Vec::new();
    s.read_to_end(&mut all).unwrap();
    let split = all.windows(4).position(|w| w == b"\r\n\r\n").expect("header end");
    (String::from_utf8_lossy(&all[..split]).into_owned(), all[split + 4..].to_vec())
}

fn json(addr: SocketAddr, method: &str, path: &str, token: Option<&str>, body: serde_json::Value) -> serde_json::Value {
    let (head, body) = request(addr, method, path, token, body.to_string().as_bytes());
    assert!(head.starts_with("HTTP/1.1 20"), "{method} {path}: {head}");
    serde_json::from_slice(&body).unwrap()
}

#[test]
fn the_process_drains_and_exits_after_the_last_request() {
    let dir = tempfile::tempdir().unwrap();
    let p = spawn(dir.path(), 1);
    let addr = p.addr;
    json(
        addr,
        "POST",
        &format!("/api/auth/invites/{}", p.invite),
        None,
        serde_json::json!({"password": "admin-pass-1"}),
    );
    let login = json(
        addr,
        "POST",
        "/api/auth/login",
        None,
        serde_json::json!({"username": "admin", "password": "admin-pass-1", "bearer": true}),
    );
    let token = login["token"].as_str().unwrap().to_owned();
    let score = json(addr, "POST", "/api/scores", Some(&token), serde_json::json!({"title": "March"}));
    let part = json(
        addr,
        "POST",
        &format!("/api/scores/{}/parts", score["id"]),
        Some(&token),
        serde_json::json!({"label": "Solo", "instrument_ids": []}),
    );
    let part = part["id"].as_i64().unwrap();

    // An open event stream (so the grace is used).
    let mut events = TcpStream::connect(addr).unwrap();
    events.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    events
        .write_all(format!("GET /api/events HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer {token}\r\n\r\n").as_bytes())
        .unwrap();
    let mut stream_text = String::new();
    let read_until = |s: &mut TcpStream, text: &mut String, what: &str| {
        let mut buf = [0u8; 4096];
        while !text.contains(what) {
            let n = s.read(&mut buf).unwrap();
            assert!(n > 0, "stream closed before {what:?}: {text}");
            text.push_str(&String::from_utf8_lossy(&buf[..n]));
        }
    };
    read_until(&mut events, &mut stream_text, "\"state\":\"ready\"");

    // An upload that is still sending when the signal comes.
    let mut file = b"%PDF-1.4\n%".to_vec();
    file.extend(std::iter::repeat_n(b'x', 4000));
    let mut upload = TcpStream::connect(addr).unwrap();
    upload.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    upload.write_all(head("PUT", &format!("/api/parts/{part}/file"), Some(&token), file.len()).as_bytes()).unwrap();
    upload.write_all(&file[..2000]).unwrap();
    std::thread::sleep(Duration::from_millis(200));

    let signalled = Instant::now();
    let pid = p.child.id().to_string();
    assert!(Command::new("kill").args(["-TERM", &pid]).status().unwrap().success());
    read_until(&mut events, &mut stream_text, "\"state\":\"shutting_down\"");
    // Within the grace: served.
    let (head1, _) = request(addr, "GET", "/api/auth/me", Some(&token), b"");
    assert!(head1.starts_with("HTTP/1.1 200"), "{head1}");
    // After the grace (1 s): refused.
    std::thread::sleep(Duration::from_millis(1300).saturating_sub(signalled.elapsed()));
    let (head2, body2) = request(addr, "GET", "/api/scores", Some(&token), b"");
    assert!(head2.starts_with("HTTP/1.1 503"), "{head2}");
    assert!(head2.to_ascii_lowercase().contains("retry-after: 5"), "{head2}");
    let e: musicus_api::ApiError = serde_json::from_slice(&body2).unwrap();
    assert_eq!(e.code, musicus_api::ErrorCode::ServerShuttingDown);
    // The upload finishes and is stored.
    upload.write_all(&file[2000..]).unwrap();
    let (head3, body3) = read_response(&mut upload);
    assert!(head3.starts_with("HTTP/1.1 200"), "{head3} {}", String::from_utf8_lossy(&body3));

    // Exits by itself, successfully, soon after the last request.
    let mut p = p;
    let deadline = Instant::now() + Duration::from_secs(8);
    let status = loop {
        if let Some(status) = p.child.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "process still running");
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(status.success(), "{status}");
    let wal = dir.path().join("musicus.sqlite-wal");
    assert!(std::fs::metadata(&wal).map_or(true, |m| m.len() == 0), "WAL checkpointed");

    // The stored upload is there after a restart.
    let p = spawn_again(dir.path());
    let got = json(p.addr, "GET", &format!("/api/parts/{part}"), Some(&token), serde_json::json!(null));
    assert_eq!(got["file"]["version"], 1, "{got}");
}

/// A second start on the same data (no invite logged: the admin exists).
fn spawn_again(data: &std::path::Path) -> Process {
    let mut child = Command::new(env!("CARGO_BIN_EXE_musicus-server"))
        .args(["serve", "--listen", "127.0.0.1:0", "--data-dir"])
        .arg(data)
        .env("RUST_LOG", "info")
        .env("NO_COLOR", "1")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let out = child.stdout.take().unwrap();
    let (tx, lines) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            let _ = tx.send(strip_ansi(&line));
        }
    });
    let listening = wait_line(&lines, "listening on http://");
    let addr = listening.split("http://").nth(1).unwrap().split_whitespace().next().unwrap().parse().unwrap();
    wait_line(&lines, "ready");
    Process { child, addr, invite: String::new(), _lines: lines }
}

#[test]
fn a_failed_start_ends_the_process_with_an_error() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("musicus.sqlite"), b"this is not a database, but long enough to be read").unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_musicus-server"))
        .args(["serve", "--listen", "127.0.0.1:0", "--data-dir"])
        .arg(dir.path())
        .env("NO_COLOR", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "process still running");
        std::thread::sleep(Duration::from_millis(50));
    };
    let mut out = String::new();
    child.stdout.take().unwrap().read_to_string(&mut out).unwrap();
    assert!(!status.success());
    assert!(out.contains("listening on"), "listened before opening the data: {out}");
}
