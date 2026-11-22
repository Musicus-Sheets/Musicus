//! Embeds the database updates (`migrations/*.sql`, `migrations/preview/*.sql`)
//! and the release list (`migrations/releases.txt`) into the program, for
//! `updates::Program::builtin` (a-update-runner). New files are picked up
//! without touching any Rust source.

use std::{env, fs, path::Path};

fn main() {
    let dir = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap()).join("migrations");
    println!("cargo:rerun-if-changed={}", dir.display());
    // Read by `option_env!` in `updates/mod.rs`; set by the release workflow.
    println!("cargo:rerun-if-env-changed=MUSICUS_RELEASE_BUILD");
    let mut files = Vec::new();
    for (sub, prefix) in [(dir.clone(), ""), (dir.join("preview"), "preview/")] {
        println!("cargo:rerun-if-changed={}", sub.display());
        let Ok(entries) = fs::read_dir(&sub) else { continue };
        for entry in entries {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "sql") {
                let name = format!("{prefix}{}", path.file_name().unwrap().to_string_lossy());
                files.push((name, path.display().to_string()));
            }
        }
    }
    files.sort();
    let mut out = String::from("pub(super) const FILES: &[(&str, &str)] = &[\n");
    for (name, path) in &files {
        out.push_str(&format!("    ({name:?}, include_str!({path:?})),\n"));
    }
    out.push_str("];\n");
    let releases = dir.join("releases.txt");
    out.push_str(&format!("pub(super) const RELEASES: &str = include_str!({:?});\n", releases.display().to_string()));
    fs::write(Path::new(&env::var("OUT_DIR").unwrap()).join("updates.rs"), out).unwrap();
}
