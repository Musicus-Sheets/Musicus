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
}

/// Tries every preset on the paths; the best fit first (most matched
/// paths, then most recognised part names, then the preset order).
pub fn detect(paths: &[String]) -> Vec<Fit> {
    let mut fits: Vec<Fit> = pattern::PRESETS
        .iter()
        .map(|preset| {
            let p = pattern::Pattern::parse(preset.pattern).expect("presets are valid");
            let (mut matched, mut recognised) = (0, 0);
            for path in paths {
                if let Some(f) = p.matches(path) {
                    matched += 1;
                    let text = parts::parse_part(&f.part, Language::De);
                    if text.instruments.iter().any(|i| i.entry.is_some()) {
                        recognised += 1;
                    }
                }
            }
            Fit { preset, matched, recognised }
        })
        .collect();
    fits.sort_by_key(|f| (std::cmp::Reverse(f.matched), std::cmp::Reverse(f.recognised)));
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

    #[test]
    fn detection_picks_the_preset_that_parses_most_files() {
        let folder = v(&[
            "Märsche/0304 Alte Kameraden - Teike/Partitur.pdf",
            "Märsche/0304 Alte Kameraden - Teike/Klarinette 1 in B.pdf",
            "0305 Böhmischer Traum/Trompete 1+2.pdf",
        ]);
        assert_eq!(detect(&folder)[0].preset.id, "folder");
        assert_eq!(detect(&folder)[0].matched, 3);
        let flat = v(&["304Stimme06_Klarinette2.pdf", "28Stimme04_KlarinetteEb.pdf", "302Stimme-06_Klarinette2.pdf"]);
        let best = &detect(&flat)[0];
        assert_eq!((best.preset.id, best.matched, best.recognised), ("flat-number", 3, 3));
        let titled = v(&["Alte Kameraden - Horn in F 1.pdf", "Alte Kameraden - Tuba.pdf"]);
        assert_eq!(detect(&titled)[0].preset.id, "flat-title");
        let category = v(&["Polkas/Böhmischer Traum/Horn.pdf", "Märsche/Alte Kameraden/Tuba.pdf"]);
        // Both folder presets parse these completely; the default comes first.
        assert_eq!(detect(&category)[0].preset.id, "folder");
        assert_eq!(detect(&category)[1].preset.id, "category");
    }

    #[test]
    fn notes_have_messages() {
        assert!(Note::AmbiguousKey("B".into()).message().contains("German"));
        let json = serde_json::to_string(&Note::ManyStaves(5)).unwrap();
        assert_eq!(json, r#"{"kind":"many_staves","detail":5}"#);
        assert_eq!(serde_json::to_string(&Note::TitleMissing).unwrap(), r#"{"kind":"title_missing"}"#);
    }
}
