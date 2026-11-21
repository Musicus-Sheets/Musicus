//! Catalogue checks (d-i18n-fluent): other languages contain no message the
//! English source lacks and use the same arguments; the complete languages
//! (German) have every English message; plural variants are categories of the
//! language; every English message is used by the web client. A message missing in English or a wrong argument
//! is already a compile error (generated functions).

#[path = "../src/scan.rs"]
mod scan;

use std::path::Path;

const ROOT: &str = env!("CARGO_MANIFEST_DIR");

/// Messages used through a lookup by code instead of their function.
const DYNAMIC_PREFIXES: &[(&str, &str)] = &[("key-name-", "t::key_name"), ("key-option-", "t::key_option(")];

/// Languages that ship complete: every English message must be translated
/// (a missing one would silently show English, R-I18N-003).
const COMPLETE: &[&str] = &["de"];

/// CLDR plural categories of each shipped language (cardinals). A select
/// variant key that is neither one of these nor a key of the English message
/// (string selectors such as `[librarian]`) is a typo.
const PLURAL_CATEGORIES: &[(&str, &[&str])] = &[("en", &["one", "other"]), ("de", &["one", "other"])];

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

#[test]
fn complete_languages_have_every_english_message() {
    let langs = scan::languages(&Path::new(ROOT).join("locales"));
    let en = catalogue(&langs[0].1);
    for lang in COMPLETE {
        let Some((_, files)) = langs.iter().find(|(l, _)| l == lang) else {
            panic!("locales/{lang} is missing");
        };
        let ids: Vec<String> = catalogue(files).into_iter().map(|m| m.id).collect();
        let missing: Vec<String> =
            en.iter().filter(|m| !ids.contains(&m.id)).map(|m| format!("{} ({})", m.id, m.file)).collect();
        assert!(missing.is_empty(), "{lang}: not translated: {missing:?}");
    }
}

#[test]
fn messages_stay_in_the_file_of_their_english_source() {
    let langs = scan::languages(&Path::new(ROOT).join("locales"));
    let en = catalogue(&langs[0].1);
    for (lang, files) in &langs[1..] {
        for m in catalogue(files) {
            if let Some(src) = en.iter().find(|e| e.id == m.id) {
                assert_eq!(src.file, m.file, "{lang}: `{}` belongs in {}", m.id, src.file);
            }
        }
    }
}

#[test]
fn variant_keys_are_plural_categories_or_english_keys() {
    let langs = scan::languages(&Path::new(ROOT).join("locales"));
    let en = catalogue(&langs[0].1);
    for (lang, files) in &langs {
        let Some((_, plurals)) = PLURAL_CATEGORIES.iter().find(|(l, _)| l == lang) else {
            panic!("{lang}: add its CLDR plural categories to PLURAL_CATEGORIES");
        };
        for m in catalogue(files) {
            let english = en.iter().find(|e| e.id == m.id).map_or(&[][..], |e| e.variant_keys.as_slice());
            for k in &m.variant_keys {
                let ok = plurals.contains(&k.as_str()) || (lang != "en" && english.contains(k)) || lang == "en";
                assert!(ok, "{lang}/{}: `{}` has the variant [{k}], not a plural category of {lang}", m.file, m.id);
            }
            for k in english {
                if !plurals.contains(&k.as_str()) {
                    assert!(m.variant_keys.contains(k), "{lang}/{}: `{}` lacks the variant [{k}]", m.file, m.id);
                }
            }
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

/// `call` occurs as a whole path (called or passed as a function).
fn uses(code: &str, call: &str) -> bool {
    code.match_indices(call)
        .any(|(i, _)| !code[i + call.len()..].starts_with(|c: char| c.is_alphanumeric() || c == '_'))
}

#[test]
fn every_english_message_is_used() {
    let mut code = String::new();
    rust_sources(&Path::new(ROOT).join("../musicus-web/src"), &mut code);
    // `name_words` uses the part-name messages for the client.
    code.push_str(&std::fs::read_to_string(Path::new(ROOT).join("src/lib.rs")).unwrap());
    let langs = scan::languages(&Path::new(ROOT).join("locales"));
    let unused: Vec<String> = catalogue(&langs[0].1)
        .into_iter()
        .filter(|m| match DYNAMIC_PREFIXES.iter().find(|(p, _)| m.id.starts_with(p)) {
            Some((_, call)) => !uses(&code, call.trim_end_matches('(')),
            None => !uses(&code, &format!("t::{}", scan::fn_name(&m.id))),
        })
        .map(|m| format!("{} ({})", m.id, m.file))
        .collect();
    assert!(unused.is_empty(), "messages not used by the web client: {unused:?}");
}
