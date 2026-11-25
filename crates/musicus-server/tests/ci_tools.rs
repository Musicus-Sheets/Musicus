//! Scripts of the continuous integration that are not part of the release pipeline: their Python
//! tests, run with `cargo test` so a broken script is noticed before CI depends on it. They need
//! Python 3; without it they are skipped.

use std::path::{Path, PathBuf};
use std::process::Command;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap().to_path_buf()
}

/// `scripts/test_test_report.py`: the test report of the workflow `Tests` (a-test-report): JUnit
/// and lcov parsing, regressions against main, the mapping of `verified_by` to tests.
#[test]
fn test_report_unit_tests() {
    let script = repo().join("scripts/test_test_report.py");
    let Some(out) = ["python3", "python"].iter().find_map(|py| Command::new(py).arg(&script).output().ok()) else {
        eprintln!("no Python: test report script not tested");
        return;
    };
    assert!(out.status.success(), "{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
}
