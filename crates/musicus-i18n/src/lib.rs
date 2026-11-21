//! Musicus message catalogue (d-i18n-fluent).
//!
//! The English catalogue `locales/en/*.ftl` is the source of truth; the build
//! script generates one function per message in [`t`] (`t::common_loading()`,
//! `t::import_files(count)`), so a missing message or a wrong argument does
//! not compile. Every language under `locales/` is compiled in; a message
//! missing in the chosen language is taken from English (R-I18N-003).
//!
//! The language is chosen once by the client ([`set_language`], from the
//! browser's preferences via [`negotiate`]); nothing is stored (R-I18N-001).
//! [`PSEUDO`] is a pseudo-locale for tests: English with accented letters,
//! longer words and every message in `⟦…⟧`, so untranslated text stands out.
//!
//! Without the default feature `bundles` the crate only exports the search
//! tables [`KEY_SPELLINGS`] and [`NAME_CONNECTORS`] (used by the server).

include!(concat!(env!("OUT_DIR"), "/messages.rs"));

/// Tag of the pseudo-locale (Chromium's accented English pseudo-locale).
pub const PSEUDO: &str = "en-XA";

/// The names of a key code in every shipped language (empty for an unknown code).
pub fn key_spellings(code: &str) -> &'static [&'static str] {
    KEY_SPELLINGS.iter().find(|(c, _)| *c == code).map_or(&[], |(_, s)| s)
}

/// Whether a (lower-case) word is a connecting word of part names in a shipped language.
pub fn is_name_connector(word: &str) -> bool {
    NAME_CONNECTORS.contains(&word.to_lowercase().as_str())
}

/// The shipped language for the browser's preferred languages
/// (`navigator.languages`): the first whose primary subtag is shipped
/// ("de-AT" ⇒ "de"), [`PSEUDO`] if asked for, else English (R-I18N-002).
/// Returns the language and the preference it matched (for `Intl` and
/// `<html lang>`; "en" when nothing matched).
pub fn negotiate<S: AsRef<str>>(preferred: &[S]) -> (&'static str, String) {
    for p in preferred {
        let p = p.as_ref().trim();
        if p.eq_ignore_ascii_case(PSEUDO) {
            return (PSEUDO, PSEUDO.to_owned());
        }
        let primary = p.split(['-', '_']).next().unwrap_or("").to_ascii_lowercase();
        if let Some(l) = LANGUAGES.iter().find(|l| **l == primary) {
            return (l, p.to_owned());
        }
    }
    ("en", "en".to_owned())
}

#[cfg(feature = "bundles")]
pub use bundles::{Arg, current_language, message, set_language};

/// The words of derived part names in the current language (a-client-part-names).
#[cfg(feature = "bundles")]
pub fn name_words() -> musicus_api::naming::NameWords {
    musicus_api::naming::NameWords {
        with_key: |i, k| t::part_name_with_key(i, k),
        no_instrument: t::part_name_no_instrument(),
        unnamed: t::part_name_unnamed(),
        instrument_sep: t::part_name_instrument_sep(),
        voice_sep: t::part_name_voice_sep(),
        key_name: t::key_name,
    }
}

#[cfg(feature = "bundles")]
mod bundles {
    use super::{PSEUDO, SOURCES};
    use fluent_bundle::{FluentArgs, FluentBundle, FluentResource, FluentValue};
    use std::borrow::Cow;
    use std::cell::RefCell;

    /// An argument of a message: text or a number (numbers select plural forms).
    #[derive(Clone, Debug, PartialEq)]
    pub enum Arg {
        Str(String),
        Num(f64),
    }

    impl From<&str> for Arg {
        fn from(s: &str) -> Self {
            Arg::Str(s.to_owned())
        }
    }
    impl From<String> for Arg {
        fn from(s: String) -> Self {
            Arg::Str(s)
        }
    }
    impl From<&String> for Arg {
        fn from(s: &String) -> Self {
            Arg::Str(s.clone())
        }
    }
    macro_rules! numbers {
        ($($t:ty),*) => {$(
            impl From<$t> for Arg {
                fn from(n: $t) -> Self {
                    Arg::Num(n as f64)
                }
            }
        )*};
    }
    numbers!(i32, i64, u32, u64, usize, f64);

    type Bundle = FluentBundle<FluentResource>;

    struct Catalogue {
        language: &'static str,
        /// The chosen language, `None` for English (and the pseudo-locale).
        chosen: Option<Bundle>,
        english: Bundle,
        pseudo: bool,
    }

    thread_local! {
        static CATALOGUE: RefCell<Option<Catalogue>> = const { RefCell::new(None) };
    }

    fn bundle(lang: &str) -> Bundle {
        let sources = SOURCES.iter().filter(|(l, _)| *l == lang).flat_map(|(_, s)| s.iter().copied());
        bundle_of(lang, sources)
    }

    fn bundle_of<'a>(lang: &str, sources: impl Iterator<Item = &'a str>) -> Bundle {
        let id: unic_langid::LanguageIdentifier = lang.parse().unwrap_or_default();
        let mut b = FluentBundle::new(vec![id]);
        // Bidi isolation marks around arguments would end up in labels and
        // tests; Musicus has no right-to-left language yet.
        b.set_use_isolating(false);
        for s in sources {
            let res = FluentResource::try_new(s.to_owned()).unwrap_or_else(|(r, _)| r);
            b.add_resource_overriding(res);
        }
        b
    }

    /// Test helper: a language with the given catalogue text.
    #[cfg(test)]
    pub(crate) fn set_test_language(language: &'static str, ftl: &str) {
        let chosen = Some(bundle_of(language, std::iter::once(ftl)));
        CATALOGUE
            .with(|c| *c.borrow_mut() = Some(Catalogue { language, chosen, english: bundle("en"), pseudo: false }));
    }

    /// Chooses the language of all messages (a shipped language or [`PSEUDO`]);
    /// anything else means English.
    pub fn set_language(language: &str) {
        let (language, _) = super::negotiate(&[language]);
        let pseudo = language == PSEUDO;
        let mut english = bundle("en");
        if pseudo {
            english.set_transform(Some(pseudo_text));
        }
        let chosen = (language != "en" && !pseudo).then(|| bundle(language));
        CATALOGUE.with(|c| *c.borrow_mut() = Some(Catalogue { language, chosen, english, pseudo }));
    }

    /// The language chosen with [`set_language`] ("en" before).
    pub fn current_language() -> &'static str {
        CATALOGUE.with(|c| c.borrow().as_ref().map_or("en", |c| c.language))
    }

    fn format(b: &Bundle, id: &str, args: &FluentArgs) -> Option<String> {
        let pattern = b.get_message(id)?.value()?;
        let mut errors = Vec::new();
        Some(b.format_pattern(pattern, Some(args), &mut errors).into_owned())
    }

    /// Formats a message: from the chosen language, else English (R-I18N-003).
    /// Used by the generated functions in [`crate::t`].
    pub fn message(id: &str, args: &[(&str, Arg)]) -> String {
        let mut fa = FluentArgs::new();
        for (k, v) in args {
            match v {
                Arg::Str(s) => fa.set(*k, FluentValue::from(s.clone())),
                Arg::Num(n) => fa.set(*k, FluentValue::from(*n)),
            }
        }
        CATALOGUE.with(|c| {
            if c.borrow().is_none() {
                *c.borrow_mut() =
                    Some(Catalogue { language: "en", chosen: None, english: bundle("en"), pseudo: false });
            }
            let c = c.borrow();
            let c = c.as_ref().unwrap();
            let text = c
                .chosen
                .as_ref()
                .and_then(|b| format(b, id, &fa))
                .or_else(|| format(&c.english, id, &fa))
                .unwrap_or_else(|| id.to_owned());
            if c.pseudo { format!("⟦{text}⟧") } else { text }
        })
    }

    /// Pseudo-locale: accented vowels/consonants and doubled vowels
    /// (≈ +30 % length), applied to the literal text of messages only (not
    /// to arguments).
    fn pseudo_text(s: &str) -> Cow<'_, str> {
        let mut out = String::with_capacity(s.len() * 2);
        for c in s.chars() {
            let m = match c {
                'a' => "àà",
                'e' => "éé",
                'i' => "îî",
                'o' => "öö",
                'u' => "üü",
                'A' => "Å",
                'E' => "É",
                'I' => "Î",
                'O' => "Ö",
                'U' => "Ü",
                'c' => "ç",
                'n' => "ñ",
                's' => "š",
                'y' => "ý",
                'C' => "Ç",
                'N' => "Ñ",
                'S' => "Š",
                _ => {
                    out.push(c);
                    continue;
                }
            };
            out.push_str(m);
        }
        Cow::Owned(out)
    }
}

#[cfg(all(test, feature = "bundles"))]
mod tests {
    use super::*;

    #[test]
    fn english_part_names_and_search_tables() {
        set_language("en");
        let w = name_words();
        let name = |i: &[&str], k: Option<&str>, v: &[&str]| musicus_api::naming::part_name(&w, None, i, k, v);
        assert_eq!(name(&["Trumpet"], Some("Bb"), &["1"]), "Trumpet in B♭ 1");
        assert_eq!(name(&["Oboe", "Flöte"], None, &["1", "2"]), "Flöte / Oboe 1 & 2");
        assert_eq!(name(&[], Some("H"), &[]), "Part in B♮");
        assert_eq!(name(&[], None, &[]), "Unnamed part");
        assert_eq!(name(&["Horn"], Some("Es/F"), &[]), "Horn in Es/F");
        assert_eq!(t::key_option("H").as_deref(), Some("B♮ (H)"));
        assert_eq!(t::key_option("Bb").as_deref(), Some("B♭"));
        assert_eq!(key_spellings("Bb"), ["B♭", "B"], "English and German names");
        assert!(is_name_connector("In"));
        assert!(!is_name_connector("clarinet"));
        for k in musicus_api::naming::KEYS {
            assert!(t::key_name(k.code).is_some(), "key {} has a name", k.code);
        }
    }

    #[test]
    fn german_part_names_and_key_names() {
        set_language("de");
        let w = name_words();
        let name = |i: &[&str], k: Option<&str>, v: &[&str]| musicus_api::naming::part_name(&w, None, i, k, v);
        assert_eq!(name(&["Klarinette"], Some("Bb"), &["1", "2"]), "Klarinette in B 1 & 2");
        assert_eq!(name(&["Horn"], Some("Eb"), &[]), "Horn in Es");
        assert_eq!(name(&["Posaune"], Some("H"), &["3"]), "Posaune in H 3");
        assert_eq!(name(&["Oboe", "Flöte"], None, &["1"]), "Flöte / Oboe 1");
        assert_eq!(name(&[], Some("F"), &["2"]), "Stimme in F 2");
        assert_eq!(name(&[], None, &[]), "Unbenannte Stimme");
        assert_eq!(name(&["Horn"], Some("Es/F"), &[]), "Horn in Es/F", "unknown legacy key as stored");
        let keys: Vec<String> =
            ["Bb", "Eb", "Ab", "Db", "Gb", "H", "F", "C"].iter().filter_map(|k| t::key_name(k)).collect();
        assert_eq!(keys, ["B", "Es", "As", "Des", "Ges", "H", "F", "C"]);
        assert_eq!(t::key_option("H").as_deref(), Some("H"), "no English hint in German");
        assert_eq!(t::annotations_set_default_name(), "Meine Anmerkungen");
        assert_eq!(current_language(), "de");
        set_language("en");
    }

    #[test]
    fn german_plurals_and_selectors() {
        set_language("de");
        assert_eq!(t::common_pages(1), "1 Seite");
        assert_eq!(t::common_pages(2), "2 Seiten");
        assert_eq!(t::common_pages(0), "0 Seiten");
        assert_eq!(t::library_parts_without_pdf(1), "1 Stimme ohne PDF");
        assert_eq!(t::import_undone(1, 3), "Rückgängig gemacht: 1 Stück und 3 Stimmen entfernt.");
        assert_eq!(t::account_user_line("Anna", "anna", "librarian"), "Anna · Benutzername anna · Rolle Notenwart");
        assert_eq!(t::import_progress_uploading(2, 5, 0, "a.pdf"), "2 von 5 werden hochgeladen: a.pdf");
        assert_eq!(
            t::import_progress_uploading(2, 5, 1, "a.pdf"),
            "2 von 5 werden hochgeladen (1 fehlgeschlagen): a.pdf"
        );
        set_language("en");
    }

    #[test]
    fn negotiation_takes_the_first_shipped_language() {
        assert_eq!(negotiate(&["fr-FR", "de-CH", "en"]), ("de", "de-CH".to_owned()), "German by language subtag");
        assert_eq!(negotiate(&["de_AT"]).0, "de");
        assert_eq!(negotiate(&["fr-FR", "en-GB", "de"]), ("en", "en-GB".to_owned()));
        assert_eq!(negotiate(&["fr-FR"]), ("en", "en".to_owned()), "English when nothing is shipped");
        assert_eq!(negotiate::<&str>(&[]), ("en", "en".to_owned()));
        assert_eq!(negotiate(&["en-xa"]).0, PSEUDO);
    }

    #[test]
    fn english_messages_and_plurals() {
        set_language("en");
        assert_eq!(t::common_loading(), "Loading…");
        assert_eq!(t::common_pages(1), "1 page");
        assert_eq!(t::common_pages(3usize), "3 pages");
        assert_eq!(t::common_pages(0), "0 pages");
        assert_eq!(t::common_size_mb("1.5"), "1.5 MB");
        assert_eq!(current_language(), "en");
    }

    #[test]
    fn missing_messages_fall_back_to_english_per_message() {
        bundles::set_test_language("de", "common-loading = Wird geladen …\n");
        assert_eq!(t::common_loading(), "Wird geladen …");
        assert_eq!(t::common_pages(2), "2 pages", "not translated ⇒ English");
        set_language("en");
    }

    #[test]
    fn pseudo_locale_marks_and_stretches_only_the_message_text() {
        set_language(PSEUDO);
        assert_eq!(t::common_loading(), "⟦Lööààdîîñg…⟧");
        assert_eq!(t::common_size_mb("Abc"), "⟦Abc MB⟧", "arguments are not changed");
        set_language("en");
    }
}
