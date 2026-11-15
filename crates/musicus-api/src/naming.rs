//! Names of parts and the keys of transposing instruments.
//!
//! A part has an optional custom name (`label`). Without one, its name is
//! derived from its instruments, key and voice (a-part-names):
//!
//! | Instruments | Key | Voice | Name |
//! | --- | --- | --- | --- |
//! | Trumpet | B♭ | 1 | `Trumpet in B♭ 1` |
//! | Flute, Piccolo | — | — | `Flute / Piccolo` |
//! | Horn | F | 3a | `Horn in F 3a` |
//! | — | — | 2 | `Part 2` |
//! | — | — | — | `Unnamed part` |
//!
//! Instruments are joined with " / " in alphabetical order. The server sends
//! the result as `Part::display_name`; clients use [`part_name`] only for a
//! live preview while editing.

use std::cmp::Ordering;

/// The keys a part may be in, as stored (`code`), shown in names (`name`)
/// and offered in the key selection (`option`), in the order of the
/// selection: the common band keys first. `None` (no key) means concert
/// pitch / not transposing.
pub const KEYS: &[Key] = &[
    Key { code: "Bb", name: "B♭", option: "B♭" },
    Key { code: "Eb", name: "E♭", option: "E♭" },
    Key { code: "F", name: "F", option: "F" },
    Key { code: "C", name: "C", option: "C" },
    Key { code: "A", name: "A", option: "A" },
    Key { code: "D", name: "D", option: "D" },
    Key { code: "G", name: "G", option: "G" },
    Key { code: "E", name: "E", option: "E" },
    Key { code: "Ab", name: "A♭", option: "A♭" },
    Key { code: "Db", name: "D♭", option: "D♭" },
    Key { code: "Gb", name: "G♭", option: "G♭" },
    // B natural has the German code "H": a stored "B" is ambiguous
    // (B-flat in German) and stays an unmapped legacy value.
    Key { code: "H", name: "B♮", option: "B♮ (H)" },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key {
    /// Stored value, ASCII (`Bb`).
    pub code: &'static str,
    /// Used in part names (`B♭`).
    pub name: &'static str,
    /// Text of the option in the key selection.
    pub option: &'static str,
}

/// The key with this stored code.
pub fn key(code: &str) -> Option<&'static Key> {
    KEYS.iter().find(|k| k.code == code)
}

/// How a stored key appears in names: the glyph form of a known key, an
/// unknown (legacy) value unchanged.
pub fn key_name(code: &str) -> &str {
    key(code).map_or(code, |k| k.name)
}

/// Name of a part: the custom name if given, otherwise derived from its
/// instruments, key and voice (see the module documentation).
pub fn part_name(custom: Option<&str>, instruments: &[&str], key: Option<&str>, voice: Option<&str>) -> String {
    if let Some(c) = custom.map(str::trim).filter(|c| !c.is_empty()) {
        return c.to_owned();
    }
    derived_name(instruments, key, voice)
}

/// The name derived from instruments, key and voice, ignoring any custom name.
pub fn derived_name(instruments: &[&str], key: Option<&str>, voice: Option<&str>) -> String {
    let key = key.map(str::trim).filter(|k| !k.is_empty());
    let voice = voice.map(str::trim).filter(|v| !v.is_empty());
    let mut names: Vec<&str> = instruments.iter().map(|n| n.trim()).filter(|n| !n.is_empty()).collect();
    names.sort_by(|a, b| natural_cmp(a, b));
    names.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    if names.is_empty() && key.is_none() && voice.is_none() {
        return "Unnamed part".into();
    }
    let mut name = if names.is_empty() { "Part".to_owned() } else { names.join(" / ") };
    if let Some(k) = key {
        name.push_str(" in ");
        name.push_str(key_name(k));
    }
    if let Some(v) = voice {
        name.push(' ');
        name.push_str(v);
    }
    name
}

/// Order for part names: case-insensitive, numbers by value
/// ("Trumpet 2" before "Trumpet 10").
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let take = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut s = String::new();
                    while let Some(c) = it.peek().copied().filter(char::is_ascii_digit) {
                        s.push(c);
                        it.next();
                    }
                    s
                };
                let (na, nb) = (take(&mut a), take(&mut b));
                let (ta, tb) = (na.trim_start_matches('0'), nb.trim_start_matches('0'));
                let ord = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            (Some(x), Some(y)) => {
                let ord = x.to_lowercase().cmp(y.to_lowercase());
                if ord != Ordering::Equal {
                    return ord;
                }
                a.next();
                b.next();
            }
        }
    }
}

/// Maps a key as people write it ("Bb", "B-flat", "in Es", "b♭", "H") to
/// its stored code. Ambiguous or unknown spellings give `None`: "B" means
/// B♭ in German and B natural in English, so it is not mapped.
/// Used by the server for API input; mirrors migration `0002`.
pub fn parse_key(text: &str) -> Option<&'static str> {
    let t = text.trim().to_lowercase();
    let t = t.strip_prefix("in ").unwrap_or(&t).trim();
    let code = match t {
        "bb" | "b♭" | "b-flat" | "b flat" | "bflat" => "Bb",
        "eb" | "e♭" | "e-flat" | "e flat" | "eflat" | "es" => "Eb",
        "ab" | "a♭" | "a-flat" | "a flat" | "aflat" | "as" => "Ab",
        "db" | "d♭" | "d-flat" | "d flat" | "dflat" | "des" => "Db",
        "gb" | "g♭" | "g-flat" | "g flat" | "gflat" | "ges" => "Gb",
        "f" => "F",
        "c" => "C",
        "a" => "A",
        "d" => "D",
        "g" => "G",
        "e" => "E",
        "h" | "b♮" | "b natural" | "bnatural" => "H",
        _ => return None,
    };
    Some(code)
}

/// A key given through the API: an exact stored code, or a spelling
/// [`parse_key`] understands.
pub fn normalize_key(text: &str) -> Option<&'static str> {
    key(text.trim()).map(|k| k.code).or_else(|| parse_key(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derived_names() {
        assert_eq!(part_name(None, &["Trumpet"], Some("Bb"), Some("1")), "Trumpet in B♭ 1");
        assert_eq!(part_name(None, &["Clarinet"], Some("Bb"), Some("2")), "Clarinet in B♭ 2");
        assert_eq!(part_name(None, &["Piccolo", "Flute"], None, None), "Flute / Piccolo");
        assert_eq!(part_name(None, &["Horn"], Some("F"), Some(" 3a ")), "Horn in F 3a");
        assert_eq!(part_name(None, &["Tuba"], None, Some("Solo")), "Tuba Solo");
        assert_eq!(part_name(None, &[], None, Some("2")), "Part 2");
        assert_eq!(part_name(None, &[], Some("Eb"), None), "Part in E♭");
        assert_eq!(part_name(None, &[], None, None), "Unnamed part");
        assert_eq!(part_name(Some("  "), &[], None, Some("")), "Unnamed part");
        // Unknown (legacy) keys are shown as stored.
        assert_eq!(part_name(None, &["Horn"], Some("Es/F"), None), "Horn in Es/F");
    }

    #[test]
    fn custom_name_wins() {
        assert_eq!(part_name(Some(" Solo Cornet "), &["Trumpet"], Some("Bb"), Some("1")), "Solo Cornet");
    }

    #[test]
    fn natural_order() {
        let mut v = vec!["Trumpet 10", "trumpet 2", "Flute", "Trumpet 1", "Trumpet 02a"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, ["Flute", "Trumpet 1", "trumpet 2", "Trumpet 02a", "Trumpet 10"]);
    }

    #[test]
    fn keys() {
        assert_eq!(parse_key("Bb"), Some("Bb"));
        assert_eq!(parse_key(" in B-flat "), Some("Bb"));
        assert_eq!(parse_key("B♭"), Some("Bb"));
        assert_eq!(parse_key("Es"), Some("Eb"));
        assert_eq!(parse_key("H"), Some("H"));
        assert_eq!(key_name("H"), "B♮");
        assert_eq!(parse_key("B"), None, "ambiguous: German B is B-flat");
        assert_eq!(parse_key("Es/F"), None);
        assert_eq!(key_name("Eb"), "E♭");
        assert_eq!(key_name("weird"), "weird");
        for k in KEYS {
            assert_eq!(parse_key(k.code), Some(k.code), "every code parses to itself");
        }
        assert_eq!(normalize_key("B"), None, "ambiguous");
        assert_eq!(normalize_key("in Bb"), Some("Bb"));
        assert_eq!(normalize_key("x"), None);
    }
}
