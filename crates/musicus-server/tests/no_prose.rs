//! The server builds no text for users (a-error-codes, R-I18N-004). `AppError`
//! takes only `ErrorCode`s (type-checked); this test also keeps error bodies
//! from being built by hand, which could put free text into `message`.

use std::{fs, path::Path};

fn sources(dir: &Path, out: &mut Vec<(String, String)>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push((path.display().to_string(), fs::read_to_string(&path).unwrap()));
        }
    }
}

#[test]
fn error_bodies_are_built_from_codes_only() {
    let mut files = Vec::new();
    sources(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut files);
    assert!(files.len() > 10);
    // `ApiError::new` / `ImportItemError::new` derive the message from the code.
    let forbidden = ["ApiError {", "ImportItemError {", "message:"];
    for (path, text) in &files {
        for (n, line) in text.lines().enumerate() {
            let code = line.split("//").next().unwrap_or("");
            for f in forbidden {
                assert!(!code.contains(f), "{path}:{}: `{f}` — send an ErrorCode instead of text", n + 1);
            }
        }
    }
}
