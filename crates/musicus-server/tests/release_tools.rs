//! The scripts of the release pipeline (a-release-pipeline, R-OPS-049): their Python tests, and
//! the smoke test of `scripts/smoke-program.py` on this build of the program (the pipeline runs it
//! on every platform's release build, R-OPS-032). Both need Python 3; without it they are skipped.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap().to_path_buf()
}

/// Runs a script of `scripts/` with the first Python found; `None` if there is none.
fn python(script: &str, args: &[&str]) -> Option<Output> {
    let script = repo().join("scripts").join(script);
    ["python3", "python"].iter().find_map(|py| Command::new(py).arg(&script).args(args).output().ok())
}

fn assert_success(out: &Output) {
    assert!(out.status.success(), "{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
}

/// `scripts/test_release_tools.py`: release check (R-OPS-040, on temporary git repositories with
/// the real merge script), title-page check (R-OPS-035), release notes, program archives and
/// `SHA256SUMS` (R-OPS-029).
#[test]
fn release_tools_unit_tests() {
    let Some(out) = python("test_release_tools.py", &[]) else {
        eprintln!("no Python: release scripts not tested");
        return;
    };
    assert_success(&out);
}

/// The pipeline's smoke test (start, web client, version, upload, marking, backup, Ctrl+C, second
/// start) against this debug build, with the web client from the test fixture.
#[test]
fn smoke_test_of_the_program() {
    let web = repo().join("crates/musicus-server/tests/fixtures/web");
    let program = env!("CARGO_BIN_EXE_musicus-server");
    let args = [
        "--program",
        program,
        "--version",
        musicus_api::VERSION,
        "--web",
        "dir",
        "--",
        "--web-dir",
        web.to_str().unwrap(),
    ];
    let Some(out) = python("smoke-program.py", &args) else {
        eprintln!("no Python: smoke test not run");
        return;
    };
    assert_success(&out);
}
