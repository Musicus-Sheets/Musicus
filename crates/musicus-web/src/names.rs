//! Names the client composes in the UI language (a-client-part-names): part
//! names from their components (the server sends none), key names, the name
//! of the unnamed annotation set, and the order of parts.

use musicus_api::{AnnotationSet, Part, naming};
use musicus_i18n::t;
use std::cmp::Ordering;

thread_local! {
    static WORDS: naming::NameWords = musicus_i18n::name_words();
}

/// Name of a part: its custom name or the name derived from its instruments,
/// key and voices with the words of the UI language.
pub fn part_name(p: &Part) -> String {
    let names: Vec<&str> = p.instruments.iter().map(|i| i.name.as_str()).collect();
    compose(p.label.as_deref(), &names, p.transposition.as_deref(), &p.voices)
}

/// A name from components (live preview while editing, import review).
pub fn compose<S: AsRef<str>>(label: Option<&str>, instruments: &[&str], key: Option<&str>, voices: &[S]) -> String {
    WORDS.with(|w| naming::part_name(w, label, instruments, key, voices))
}

/// Name of the review part (the name it will have after the commit).
pub fn review_name(p: &musicus_api::import::review::ReviewPart) -> String {
    WORDS.with(|w| p.name(w))
}

/// Entry of a key in the key selection ("B♮ (H)").
pub fn key_option(code: &str) -> String {
    t::key_option(code).unwrap_or_else(|| code.to_owned())
}

/// Order of part names: by name, numbers by value.
pub fn cmp_names(a: &str, b: &str) -> Ordering {
    naming::natural_cmp(a, b)
}

/// The parts in display order (by name; the server sends them by id).
pub fn sorted(parts: &[Part]) -> Vec<&Part> {
    let mut v: Vec<(String, &Part)> = parts.iter().map(|p| (part_name(p), p)).collect();
    v.sort_by(|a, b| cmp_names(&a.0, &b.0));
    v.into_iter().map(|(_, p)| p).collect()
}

/// Name of an annotation set; the unnamed one is "My annotations" in the UI language.
pub fn set_name(set: &AnnotationSet) -> String {
    set.name.clone().unwrap_or_else(t::annotations_set_default_name)
}
