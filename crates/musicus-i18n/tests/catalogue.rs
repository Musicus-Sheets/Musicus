//! Catalogue checks (d-i18n-fluent): other languages contain no message the
//! English source lacks and use the same arguments; every English message is
//! used by the web client. A message missing in English or a wrong argument
//! is already a compile error (generated functions).

#[path = "../src/scan.rs"]
mod scan;

use std::path::Path;

const ROOT: &str = env!("CARGO_MANIFEST_DIR");

/// Messages used through a lookup by code instead of their function.
const DYNAMIC_PREFIXES: &[(&str, &str)] = &[("key-name-", "t::key_name(")];

fn catalogue(lang_files: &[std::path::PathBuf]) -> Vec<scan::Msg> {
    lang_files.iter().flat_map(|f| scan::messages(f).unwrap()).collect()
}

#[test]
fn other_languages_match_the_english_messages() {
    let langs = scan::languages(&Path::new(ROOT).join("locales"));
    let en = catalogue(&langs[0].1);
    for (lang, files) in &langs[1..] {
        for m in catalogue(files) {
            let Some(src) = en.iter().find(|e| e.id == m.id) else {
                panic!("{lang}/{}: `{}` is not in the English catalogue (obsolete?)", m.file, m.id);
            };
            let (mut a, mut b) = (src.vars.clone(), m.vars.clone());
            a.sort();
            b.sort();
            assert_eq!(a, b, "{lang}/{}: `{}` uses other arguments than English", m.file, m.id);
        }
    }
}

fn rust_sources(dir: &Path, out: &mut String) {
    for e in std::fs::read_dir(dir).unwrap().filter_map(Result::ok) {
        let p = e.path();
        if p.is_dir() {
            rust_sources(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push_str(&std::fs::read_to_string(&p).unwrap());
        }
    }
}

#[test]
fn every_english_message_is_used() {
    let mut code = String::new();
    rust_sources(&Path::new(ROOT).join("../musicus-web/src"), &mut code);
    let langs = scan::languages(&Path::new(ROOT).join("locales"));
    let unused: Vec<String> = catalogue(&langs[0].1)
        .into_iter()
        .filter(|m| match DYNAMIC_PREFIXES.iter().find(|(p, _)| m.id.starts_with(p)) {
            Some((_, call)) => !code.contains(call),
            None => !code.contains(&format!("t::{}(", scan::fn_name(&m.id))),
        })
        .map(|m| format!("{} ({})", m.id, m.file))
        .collect();
    assert!(unused.is_empty(), "messages not used by the web client: {unused:?}");
}
