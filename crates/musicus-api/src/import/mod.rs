//! Mass import of a sheet archive (phase 1: folder structure and file names).
//!
//! Pure functions shared by the server (which stores and commits the
//! proposals) and the web client (which previews them while the librarian
//! chooses a pattern), so both always agree (a-import-shared):
//!
//! - [`pattern`]: which part of a path is number, title, composer, … and
//!   which is the part name; presets and their detection ([`detect`]).
//! - [`parts`]: instrument, key and voices from a part name (German/English,
//!   abbreviations); the instrument dictionary.
//! - [`review`]: grouping files into pieces ([`review::propose`]) and
//!   resolving a proposal against the library (instruments, stored aliases,
//!   existing pieces, groups) into colours: green = sure, yellow = check,
//!   red = blocks the commit.
//!
//! Every value carries its [`Source`]. Phase 1 knows `pattern`, `dictionary`,
//! `alias`, `library` and `librarian`; a later OCR of the title page or an
//! inventory list (CSV) add candidates with the sources `ocr` / `inventory`
//! without changing the workflow.

pub mod parts;
pub mod pattern;
pub mod review;

use serde::{Deserialize, Serialize};

/// Naming language of a batch: decides German key letters ("B" = B♭, "Es",
/// "As", "H") in part names (a-import-keys).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    De,
    #[default]
    En,
}

impl Language {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::De => "de",
            Self::En => "en",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "de" => Some(Self::De),
            "en" => Some(Self::En),
            _ => None,
        }
    }
}

/// Confidence of a value: green = sure (pattern + dictionary word or stored
/// alias), yellow = please check (abbreviation, guessed, ambiguous, a similar
/// piece exists), red = unknown or contradictory, blocks the commit.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    #[default]
    Green,
    Yellow,
    Red,
}

/// Where a value comes from.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// The file or folder name, through the pattern.
    Pattern,
    /// The built-in instrument dictionary.
    Dictionary,
    /// An alias a librarian stored earlier (`instrument_aliases`).
    Alias,
    /// The library itself (an instrument with exactly this name, an existing piece).
    Library,
    /// A librarian typed or chose it in the review.
    Librarian,
    /// Future: an inventory list (CSV) of the archive.
    Inventory,
    /// Future: text recognised on the first page.
    Ocr,
}

/// Why a value is yellow or red; `message()` is the text the review shows.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
pub enum Note {
    AmbiguousKey(String),
    UnknownKey(String),
    InvalidKey(String),
    TwoKeys(String),
    SeveralVoices(String),
    Abbreviation(String),
    Guessed(String),
    AmbiguousInstrument(String),
    UnknownInstrument(String),
    NoInstrument,
    Unrecognised(String),
    Contradictory(String),
    TitleMissing,
    SimilarTitle(String),
    ConfirmTarget(String),
    TargetGone,
    NoParts,
    NotUploaded,
    UploadFailed(String),
    ScoreWithOneStaff,
    ManyStaves(u32),
    NoReaders,
}

impl Note {
    pub fn message(&self) -> String {
        match self {
            Self::AmbiguousKey(k) => {
                format!(
                    "\"{k}\" is a key name in German only (B = B♭, Es = E♭, H = B♮): choose the key, or set the batch language to German"
                )
            }
            Self::UnknownKey(k) => format!("\"{k}\" is not a known key"),
            Self::InvalidKey(k) => format!("unknown key \"{k}\""),
            Self::TwoKeys(k) => format!("several keys in the name: {k}"),
            Self::SeveralVoices(v) => {
                format!("numbers without a separator in the name ({v}): check the voices (also a copy or page number?)")
            }
            Self::Abbreviation(t) => format!("\"{t}\" is an abbreviation: check the instrument"),
            Self::Guessed(t) => format!("\"{t}\" was guessed from the beginning of the word: check the instrument"),
            Self::AmbiguousInstrument(t) => format!("\"{t}\" can mean several instruments: check it"),
            Self::UnknownInstrument(t) => format!("unknown instrument \"{t}\": choose an instrument or create it"),
            Self::NoInstrument => "no instrument: nobody could get this part through an instrument grant".into(),
            Self::Unrecognised(t) => format!("not understood: \"{t}\""),
            Self::Contradictory(v) => format!("the files of this piece disagree: {v}"),
            Self::TitleMissing => "the file names contain no title: type it".into(),
            Self::SimilarTitle(t) => format!("a piece with a similar title exists: {t}"),
            Self::ConfirmTarget(t) => {
                format!("this may be the existing piece {t}: add the parts to it or import as a new piece")
            }
            Self::TargetGone => "the chosen existing piece was deleted".into(),
            Self::NoParts => "no parts to import".into(),
            Self::NotUploaded => "file not uploaded yet: select the folder again to continue".into(),
            Self::UploadFailed(m) => format!("upload failed: {m}"),
            Self::ScoreWithOneStaff => "named as full score, but its systems have only one staff".into(),
            Self::ManyStaves(n) => format!("a single part, but its systems have {n} staves: a score?"),
            Self::NoReaders => "no group can read this part yet".into(),
        }
    }
}

/// Splits off the selected folder: the browser reports `Archive/Marches/…`;
/// if every path starts with the same folder, it is returned separately and
/// removed from the paths.
pub fn strip_common_root(paths: &[String]) -> (Option<String>, Vec<String>) {
    let first = paths.first().and_then(|p| p.split_once('/')).map(|(r, _)| r.to_owned());
    match first {
        Some(root) if paths.iter().all(|p| p.starts_with(&format!("{root}/"))) => {
            let rest = paths.iter().map(|p| p[root.len() + 1..].to_owned()).collect();
            (Some(root), rest)
        }
        _ => (None, paths.to_vec()),
    }
}

/// Is this file a candidate for the import (PDF by its name)?
pub fn is_pdf_name(path: &str) -> bool {
    let n = path.len();
    n > 4 && path.is_char_boundary(n - 4) && path[n - 4..].eq_ignore_ascii_case(".pdf")
}

/// How well a preset fits a file list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fit {
    pub preset: &'static pattern::Preset,
    /// Paths the pattern matches.
    pub matched: usize,
    /// Of those, part names with a known instrument word.
    pub recognised: usize,
    /// Of those, part names understood completely: every word is an
    /// instrument, key, voice or filler word (no leftover title words).
    pub understood: usize,
}

/// Tries every preset on the paths; the best fit first: most matched
/// paths, then most recognised part names, then most part names
/// understood completely, then the preset order (a-import-patterns).
pub fn detect(paths: &[String]) -> Vec<Fit> {
    let mut fits: Vec<Fit> = pattern::PRESETS
        .iter()
        .map(|preset| {
            let p = pattern::Pattern::parse(preset.pattern).expect("presets are valid");
            let (mut matched, mut recognised, mut understood) = (0, 0, 0);
            for path in paths {
                if let Some(f) = p.matches(path) {
                    matched += 1;
                    let text = parts::parse_part(&f.part, Language::De);
                    if text.instruments.iter().any(|i| i.entry.is_some()) {
                        recognised += 1;
                        if text.rest.is_empty() && text.instruments.iter().all(|i| i.entry.is_some()) {
                            understood += 1;
                        }
                    }
                }
            }
            Fit { preset, matched, recognised, understood }
        })
        .collect();
    // Stable sort: ties keep the preset order.
    fits.sort_by_key(|f| {
        (std::cmp::Reverse(f.matched), std::cmp::Reverse(f.recognised), std::cmp::Reverse(f.understood))
    });
    fits
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn root_folder_is_split_off() {
        let (root, rest) = strip_common_root(&v(&["Archiv/A/x.pdf", "Archiv/B/y.pdf"]));
        assert_eq!((root.as_deref(), rest), (Some("Archiv"), v(&["A/x.pdf", "B/y.pdf"])));
        let (root, rest) = strip_common_root(&v(&["x.pdf", "Archiv/y.pdf"]));
        assert_eq!((root, rest.len()), (None, 2));
        assert!(is_pdf_name("a/B.PDF") && !is_pdf_name("a/notes.txt") && !is_pdf_name(".pdf"));
    }

    fn best(paths: &[&str]) -> &'static str {
        detect(&v(paths))[0].preset.id
    }

    #[test]
    fn detection_picks_the_preset_that_parses_most_files() {
        let folder = v(&[
            "Marches/0304 Radetzky March - Strauss/Full score.pdf",
            "Marches/0304 Radetzky March - Strauss/Clarinet 1 in Bb.pdf",
            "0305 Bohemian Dream/Trumpet 1+2.pdf",
            "Polkas/Bohemian Dream/Horn.pdf",
        ]);
        let fit = &detect(&folder)[0];
        assert_eq!((fit.preset.id, fit.matched, fit.recognised), ("folder", 4, 4));
        // Archive number and part, with any of the usual separators.
        for flat in [
            ["0304 Trumpet 1.pdf", "0304 Tuba.pdf", "28 Flute.pdf"],
            ["0304_Trumpet_1.pdf", "0304_Tuba.pdf", "28_Flute.pdf"],
            ["0304 - Trumpet 1.pdf", "0304 - Tuba.pdf", "28 - Flute.pdf"],
        ] {
            let fit = &detect(&v(&flat))[0];
            assert_eq!((fit.preset.id, fit.matched, fit.understood), ("flat-number", 3, 3), "{flat:?}");
        }
        // Title and part, with or without number.
        assert_eq!(best(&["Radetzky March - Horn in F 1.pdf", "Radetzky March - Tuba.pdf"]), "flat-title");
        assert_eq!(best(&["0304 Radetzky March - Horn.pdf", "0304 Radetzky March - Tuba.pdf"]), "flat-title");
        assert_eq!(best(&["0304 - Radetzky March - Horn.pdf", "28 - Bohemian Dream - Tuba.pdf"]), "flat-title");
        assert_eq!(best(&["0304_Radetzky_March_-_Horn.pdf"]), "flat-title");
    }

    #[test]
    fn detection_ties_are_decided_sensibly_and_deterministically() {
        // "0304 Radetzky March - Horn" fits "{number} {part}" too, but then the
        // part name keeps the title words: the title preset understands it fully.
        let titled = v(&["0304 Radetzky March - Horn.pdf"]);
        let fits = detect(&titled);
        let flat = fits.iter().find(|f| f.preset.id == "flat-number").unwrap();
        assert_eq!((flat.matched, flat.recognised, flat.understood), (1, 1, 0));
        assert_eq!(fits[0].preset.id, "flat-title");
        // "0304 - Horn" fits both completely (title "0304" or number 0304):
        // the preset order decides, the number wins.
        let fits = detect(&v(&["0304 - Horn.pdf"]));
        assert_eq!((fits[0].matched, fits[0].understood, fits[1].matched, fits[1].understood), (1, 1, 1, 1));
        assert_eq!((fits[0].preset.id, fits[1].preset.id), ("flat-number", "flat-title"));
        // Mixed: the preset that fits more files wins.
        assert_eq!(best(&["0304 - Horn.pdf", "Radetzky March - Tuba.pdf"]), "flat-title");
        // Nothing fits: every preset with 0, in the preset order.
        let none = detect(&v(&["Horn.pdf"]));
        assert!(none.iter().all(|f| f.matched == 0));
        assert_eq!(none.iter().map(|f| f.preset.id).collect::<Vec<_>>(), ["folder", "flat-number", "flat-title"]);
    }

    #[test]
    fn notes_have_messages() {
        assert!(Note::AmbiguousKey("B".into()).message().contains("German"));
        let json = serde_json::to_string(&Note::ManyStaves(5)).unwrap();
        assert_eq!(json, r#"{"kind":"many_staves","detail":5}"#);
        assert_eq!(serde_json::to_string(&Note::TitleMissing).unwrap(), r#"{"kind":"title_missing"}"#);
    }
}
