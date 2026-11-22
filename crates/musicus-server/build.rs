//! Embeds the database updates (`migrations/*.sql`, `migrations/preview/*.sql`)
//! and the release list (`migrations/releases.txt`) into the program, for
//! `updates::Program::builtin` (a-update-runner). New files are picked up
//! without touching any Rust source.
//!
//! With the feature `embed-web` it also embeds the built web client
//! (`web::builtin`, R-OPS-030, a-embedded-web).

use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn main() {
    migrations();
    if env::var_os("CARGO_FEATURE_EMBED_WEB").is_some() {
        web_client();
    }
}

fn migrations() {
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

/// Default folder of the built client (output of `dx bundle --release`),
/// relative to the workspace root like a relative `MUSICUS_WEB_DIST`.
const DEFAULT_WEB_DIST: &str = "target/dx/musicus-web/release/web/public";

fn web_client() {
    println!("cargo:rerun-if-env-changed=MUSICUS_WEB_DIST");
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let workspace = manifest.parent().and_then(Path::parent).unwrap();
    let dist = env::var_os("MUSICUS_WEB_DIST").map_or_else(|| PathBuf::from(DEFAULT_WEB_DIST), PathBuf::from);
    let dist = workspace.join(dist);
    println!("cargo:rerun-if-changed={}", dist.display());
    if !dist.join("index.html").is_file() {
        panic!(
            "feature embed-web: {} holds no built web client (index.html missing). Build it first with\n  \
             ./scripts/fetch-pdfjs.sh && dx bundle --platform web --package musicus-web --release\n\
             or set MUSICUS_WEB_DIST to the folder of a built client (relative to the repository root).",
            dist.display()
        );
    }
    let mut files = Vec::new();
    collect(&dist, &dist, &mut files);
    files.sort();
    let is_copy = |p: &str| {
        [".br", ".gz"].iter().any(|ext| {
            p.strip_suffix(ext).is_some_and(|base| files.binary_search_by(|(f, _)| f.as_str().cmp(base)).is_ok())
        })
    };
    let include = |rel: &str| {
        let path = dist.join(rel);
        path.is_file().then(|| format!("Some(include_bytes!({:?}))", path.display().to_string()))
    };
    let mut out = String::from("pub(super) static ASSETS: &[super::Asset] = &[\n");
    for (rel, path) in files.iter().filter(|(rel, _)| !is_copy(rel)) {
        let body = fs::read(path).unwrap();
        out.push_str(&format!(
            "    super::Asset {{ path: {rel:?}, body: include_bytes!({:?}), br: {}, gzip: {}, hash: \"{:016x}\" }},\n",
            path.display().to_string(),
            include(&format!("{rel}.br")).unwrap_or_else(|| "None".into()),
            include(&format!("{rel}.gz")).unwrap_or_else(|| "None".into()),
            fnv1a(&body),
        ));
    }
    out.push_str("];\n");
    fs::write(Path::new(&env::var("OUT_DIR").unwrap()).join("web.rs"), out).unwrap();
}

/// Files below `dir` as (`a/b.js` relative to `root`, full path).
fn collect(root: &Path, dir: &Path, files: &mut Vec<(String, PathBuf)>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect(root, &path, files);
        } else {
            let rel = path.strip_prefix(root).unwrap();
            let rel: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
            files.push((rel.join("/"), path));
        }
    }
}

/// 64-bit FNV-1a: enough to tell versions of a file apart for the ETag.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3))
}
