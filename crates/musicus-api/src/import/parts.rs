//! Part names in file names: instrument(s), key and voices, in any order,
//! German or English, abbreviated or not ("Klarinette 2 in B",
//! "Bb Clarinet II", "Klar2B", "KlarinetteEb", "Trp 1", "Pos. 3",
//! "Trompete 1+2", "Horn 1/2", "Flügelhorn I & II").
//!
//! The parser only reads the text; which instrument of the library a word
//! means is decided later ([`super::review`]) with the library's instruments
//! and the stored aliases. A key is never assumed: no key in the name ⇒ no
//! key. German key names ("B" = B♭, "Es", "As", "Des", "Ges", "H" = B♮) are
//! read only when the batch's naming language is German; otherwise such a
//! letter is reported as ambiguous and left empty (a-import-keys).
//!
//! A part may contain several voices (a-part-voices). Voices joined by an
//! explicit separator ("+", "&", "/", ",", "und", "and") are sure; numbers
//! that merely stand next to each other ("Trompete 1 2") are read as voices
//! too but flagged, because file names also carry copy or page numbers
//! (a-import-voices).

use super::{Language, Level, Note};
use serde::{Deserialize, Serialize};

/// A dictionary instrument: the name proposed when it has to be created and
/// the spellings that mean it. Aliases are folded (see [`fold`]).
pub struct Entry {
    pub name: &'static str,
    /// Full words: (alias, language). `None` = the same in both languages.
    pub words: &'static [(&'static str, Option<Language>)],
    /// Abbreviations: recognised, but shown yellow until confirmed.
    pub abbreviations: &'static [&'static str],
    /// The word alone is ambiguous (e.g. "Bass"): always yellow.
    pub ambiguous: bool,
}

use Language::{De, En};

/// The instrument dictionary. "Partitur" (full score) and "Direktion"
/// (conductor's part, condensed score) are two different instruments
/// (a-import-dictionary): some pieces have both.
pub const DICTIONARY: &[Entry] = &[
    Entry {
        name: "Partitur",
        words: &[("partitur", Some(De)), ("score", Some(En)), ("full score", Some(En))],
        abbreviations: &[],
        ambiguous: false,
    },
    Entry {
        name: "Direktion",
        words: &[
            ("direktion", Some(De)),
            ("dirigierstimme", Some(De)),
            ("direction", Some(En)),
            ("conductor", Some(En)),
            ("conductor score", Some(En)),
            ("condensed score", Some(En)),
        ],
        abbreviations: &["dir", "cond"],
        ambiguous: false,
    },
    Entry {
        name: "Piccolo",
        words: &[("piccolo", None), ("pikkolo", Some(De)), ("piccoloflote", Some(De)), ("piccolo flute", Some(En))],
        abbreviations: &["picc", "pic", "pikk"],
        ambiguous: false,
    },
    Entry {
        name: "Flute",
        words: &[
            ("flute", Some(En)),
            ("flote", Some(De)),
            ("floete", Some(De)),
            ("querflote", Some(De)),
            ("querfloete", Some(De)),
        ],
        abbreviations: &["fl", "flt"],
        ambiguous: false,
    },
    Entry { name: "Oboe", words: &[("oboe", None)], abbreviations: &["ob"], ambiguous: false },
    Entry {
        name: "English Horn",
        words: &[
            ("english horn", Some(En)),
            ("cor anglais", Some(En)),
            ("englischhorn", Some(De)),
            ("englisch horn", Some(De)),
        ],
        abbreviations: &["ehn"],
        ambiguous: false,
    },
    Entry {
        name: "Bassoon",
        words: &[("bassoon", Some(En)), ("fagott", Some(De))],
        abbreviations: &["bsn", "fg", "fag"],
        ambiguous: false,
    },
    Entry {
        name: "Bass Clarinet",
        words: &[("bass clarinet", Some(En)), ("bassklarinette", Some(De)), ("bass klarinette", Some(De))],
        abbreviations: &["bcl", "bkl", "bass clar", "bass klar"],
        ambiguous: false,
    },
    Entry {
        name: "Alto Clarinet",
        words: &[("alto clarinet", Some(En)), ("altklarinette", Some(De)), ("alt klarinette", Some(De))],
        abbreviations: &[],
        ambiguous: false,
    },
    Entry {
        name: "Clarinet",
        words: &[("clarinet", Some(En)), ("klarinette", Some(De))],
        abbreviations: &["clar", "cl", "clt", "klar", "kl", "klt"],
        ambiguous: false,
    },
    Entry {
        name: "Soprano Saxophone",
        words: &[
            ("soprano saxophone", Some(En)),
            ("soprano sax", Some(En)),
            ("sopransaxophon", Some(De)),
            ("sopran saxophon", Some(De)),
            ("sopransax", Some(De)),
            ("sopran sax", Some(De)),
        ],
        abbreviations: &["ssax", "sop sax"],
        ambiguous: false,
    },
    Entry {
        name: "Alto Saxophone",
        words: &[
            ("alto saxophone", Some(En)),
            ("alto sax", Some(En)),
            ("altsaxophon", Some(De)),
            ("alt saxophon", Some(De)),
            ("altsax", Some(De)),
            ("alt sax", Some(De)),
        ],
        abbreviations: &["asax", "a sax"],
        ambiguous: false,
    },
    Entry {
        name: "Tenor Saxophone",
        words: &[
            ("tenor saxophone", Some(En)),
            ("tenor sax", None),
            ("tenorsaxophon", Some(De)),
            ("tenor saxophon", Some(De)),
            ("tenorsax", Some(De)),
        ],
        abbreviations: &["tsax", "t sax", "ten sax"],
        ambiguous: false,
    },
    Entry {
        name: "Baritone Saxophone",
        words: &[
            ("baritone saxophone", Some(En)),
            ("baritone sax", Some(En)),
            ("bari sax", Some(En)),
            ("baritonsaxophon", Some(De)),
            ("bariton saxophon", Some(De)),
            ("baritonsax", Some(De)),
            ("bariton sax", Some(De)),
        ],
        abbreviations: &["bsax", "barisax", "bar sax"],
        ambiguous: false,
    },
    Entry {
        name: "Saxophone",
        words: &[("saxophone", Some(En)), ("saxophon", Some(De))],
        abbreviations: &["sax"],
        ambiguous: false,
    },
    Entry {
        name: "Horn",
        words: &[("horn", None), ("french horn", Some(En)), ("waldhorn", Some(De))],
        abbreviations: &["hn", "hr", "hrn"],
        ambiguous: false,
    },
    Entry {
        name: "Trumpet",
        words: &[("trumpet", Some(En)), ("trompete", Some(De))],
        abbreviations: &["trp", "tpt", "trpt", "tp", "tr", "tromp"],
        ambiguous: false,
    },
    Entry {
        name: "Cornet",
        words: &[("cornet", Some(En)), ("kornett", Some(De))],
        abbreviations: &["cnt", "crt", "kor"],
        ambiguous: false,
    },
    Entry {
        name: "Flugelhorn",
        words: &[("flugelhorn", None), ("fluegelhorn", Some(De)), ("flugel horn", Some(En))],
        abbreviations: &["flgh", "flhn", "flgl", "flh"],
        ambiguous: false,
    },
    Entry {
        name: "Bass Trombone",
        words: &[("bass trombone", Some(En)), ("bassposaune", Some(De)), ("bass posaune", Some(De))],
        abbreviations: &["btbn", "bpos", "bass pos"],
        ambiguous: false,
    },
    Entry {
        name: "Trombone",
        words: &[("trombone", Some(En)), ("posaune", Some(De))],
        abbreviations: &["trb", "tbn", "trbn", "pos"],
        ambiguous: false,
    },
    Entry {
        name: "Alto Horn",
        words: &[("alto horn", Some(En)), ("althorn", Some(De))],
        abbreviations: &[],
        ambiguous: false,
    },
    Entry {
        name: "Tenor Horn",
        words: &[("tenor horn", Some(En)), ("tenorhorn", Some(De))],
        abbreviations: &["thn", "thr"],
        ambiguous: false,
    },
    Entry {
        name: "Baritone",
        words: &[("baritone", Some(En)), ("bariton", Some(De))],
        abbreviations: &["bar", "bari"],
        ambiguous: false,
    },
    Entry { name: "Euphonium", words: &[("euphonium", None)], abbreviations: &["euph", "eup"], ambiguous: false },
    Entry {
        name: "Tuba",
        words: &[("tuba", None), ("tuben", Some(De)), ("basstuba", Some(De)), ("bass tuba", Some(En))],
        abbreviations: &["tba"],
        ambiguous: false,
    },
    Entry { name: "Bass", words: &[("bass", None)], abbreviations: &[], ambiguous: true },
    Entry {
        name: "String Bass",
        words: &[
            ("string bass", Some(En)),
            ("double bass", Some(En)),
            ("kontrabass", Some(De)),
            ("contrabass", Some(En)),
        ],
        abbreviations: &["kb", "cb"],
        ambiguous: false,
    },
    Entry {
        name: "Electric Bass",
        words: &[("electric bass", Some(En)), ("bass guitar", Some(En)), ("bassgitarre", Some(De))],
        abbreviations: &["ebass"],
        ambiguous: false,
    },
    Entry {
        name: "Timpani",
        words: &[("timpani", Some(En)), ("pauken", Some(De)), ("pauke", Some(De))],
        abbreviations: &["timp", "pk"],
        ambiguous: false,
    },
    Entry {
        name: "Percussion",
        words: &[("percussion", Some(En)), ("schlagwerk", Some(De)), ("schlagzeug", Some(De))],
        abbreviations: &["perc", "schlz", "schlgz"],
        ambiguous: false,
    },
    Entry {
        name: "Drum Set",
        words: &[("drum set", Some(En)), ("drumset", Some(En)), ("drums", Some(En)), ("drum kit", Some(En))],
        abbreviations: &["dr"],
        ambiguous: false,
    },
    Entry {
        name: "Snare Drum",
        words: &[("snare drum", Some(En)), ("snare", Some(En)), ("kleine trommel", Some(De))],
        abbreviations: &["sd", "kl tr"],
        ambiguous: false,
    },
    Entry {
        name: "Bass Drum",
        words: &[("bass drum", Some(En)), ("grosse trommel", Some(De))],
        abbreviations: &["bd", "gr tr"],
        ambiguous: false,
    },
    Entry {
        name: "Cymbals",
        words: &[("cymbals", Some(En)), ("becken", Some(De))],
        abbreviations: &["cym", "bck"],
        ambiguous: false,
    },
    Entry {
        name: "Glockenspiel",
        words: &[("glockenspiel", None), ("lyra", Some(De)), ("bells", Some(En))],
        abbreviations: &["glsp", "glock"],
        ambiguous: false,
    },
    Entry {
        name: "Mallets",
        words: &[
            ("mallets", Some(En)),
            ("xylophone", Some(En)),
            ("xylophon", Some(De)),
            ("vibraphone", Some(En)),
            ("vibraphon", Some(De)),
            ("marimba", None),
        ],
        abbreviations: &["xyl", "vib"],
        ambiguous: false,
    },
    Entry {
        name: "Piano",
        words: &[("piano", None), ("klavier", Some(De))],
        abbreviations: &["pno", "pf", "klav"],
        ambiguous: false,
    },
    Entry {
        name: "Guitar",
        words: &[("guitar", Some(En)), ("gitarre", Some(De))],
        abbreviations: &["gtr", "git"],
        ambiguous: false,
    },
    Entry {
        name: "Harp",
        words: &[("harp", Some(En)), ("harfe", Some(De))],
        abbreviations: &["hp", "hrf"],
        ambiguous: false,
    },
    Entry {
        name: "Violin",
        words: &[("violin", Some(En)), ("violine", Some(De)), ("geige", Some(De))],
        abbreviations: &["vl", "vln", "vn"],
        ambiguous: false,
    },
    Entry {
        name: "Viola",
        words: &[("viola", None), ("bratsche", Some(De))],
        abbreviations: &["vla", "va"],
        ambiguous: false,
    },
    Entry {
        name: "Cello",
        words: &[("cello", None), ("violoncello", None), ("violoncelli", None), ("celli", None)],
        abbreviations: &["vc", "vlc"],
        ambiguous: false,
    },
];

/// Words that carry no information in a part name.
const FILLER: &[&str] =
    &["stimme", "stimmen", "part", "voice", "st", "nd", "rd", "th", "fur", "for", "the", "der", "die", "das", "im"];

/// Lower case with diacritics removed (`Flöte` ⇒ `flote`, `ß` ⇒ `ss`), the
/// form dictionary entries and stored aliases are compared in.
pub fn fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            'ä' | 'Ä' | 'á' | 'à' | 'â' | 'Á' | 'À' | 'Â' | 'å' => out.push('a'),
            'ö' | 'Ö' | 'ó' | 'ò' | 'ô' | 'Ó' | 'Ò' | 'Ô' | 'ø' => out.push('o'),
            'ü' | 'Ü' | 'ú' | 'ù' | 'û' | 'Ú' | 'Ù' | 'Û' => out.push('u'),
            'é' | 'è' | 'ê' | 'ë' | 'É' | 'È' | 'Ê' => out.push('e'),
            'í' | 'ì' | 'î' | 'ï' => out.push('i'),
            'č' | 'ç' | 'Č' => out.push('c'),
            'š' | 'Š' => out.push('s'),
            'ž' | 'Ž' => out.push('z'),
            'ß' => out.push_str("ss"),
            c => out.extend(c.to_lowercase()),
        }
    }
    out
}

/// The key under which an instrument word is stored as alias: its words
/// folded, joined by single spaces ("Bass Klar." ⇒ "bass klar").
pub fn alias_key(text: &str) -> String {
    tokenize(text).iter().filter(|t| *t != "+").map(|t| fold(t)).collect::<Vec<_>>().join(" ")
}

/// Splits a part name into words: at spaces and punctuation, between a
/// lower- and an upper-case letter (`KlarinetteEb`), and between letters and
/// digits (`Klar2B`), but a digit followed by one lower-case letter stays
/// together (`3a` is a voice). The separators `+`, `&`, `/` and `,` are
/// kept as the word `+` (they join voices: "1/2", "I & II").
pub fn tokenize(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut tokens: Vec<String> = Vec::new();
    let mut cur = String::new();
    let flush = |cur: &mut String, tokens: &mut Vec<String>| {
        if !cur.is_empty() {
            tokens.push(std::mem::take(cur));
        }
    };
    for (i, &c) in chars.iter().enumerate() {
        let word = c.is_alphanumeric() || c == '♭';
        if !word {
            flush(&mut cur, &mut tokens);
            if matches!(c, '+' | '&' | '/' | ',') {
                tokens.push("+".into());
            }
            continue;
        }
        if let Some(&prev) = cur.chars().last().as_ref() {
            let boundary = (prev.is_lowercase() && c.is_uppercase())
                || (prev.is_alphabetic() && c.is_ascii_digit())
                || (prev.is_ascii_digit() && c.is_alphabetic() && {
                    // "3a" stays one voice; "2B" or "1st" split.
                    let next_is_letter = chars.get(i + 1).is_some_and(|n| n.is_alphabetic());
                    !(c.is_lowercase() && !next_is_letter)
                });
            if boundary {
                flush(&mut cur, &mut tokens);
            }
        }
        cur.push(c);
    }
    flush(&mut cur, &mut tokens);
    tokens
}

/// An instrument word found in a part name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstrumentText {
    /// As written in the file name, e.g. "Klar".
    pub text: String,
    /// Folded words, the key for stored aliases, e.g. "klar".
    pub alias: String,
    /// Dictionary instrument, e.g. "Clarinet"; `None` = unknown word.
    pub entry: Option<String>,
    /// How sure the dictionary is (green = full word, yellow = abbreviation,
    /// guessed from a word start, or ambiguous word; red = unknown).
    pub level: Level,
}

/// A key read from the part name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Reading {
    pub value: Option<String>,
    pub level: Level,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<Note>,
}

/// The voices read from a part name, in the order written, without
/// duplicates (`naming::clean_voices`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Voices {
    pub values: Vec<String>,
    /// Green, or yellow when numbers stand next to each other without a
    /// separator ("Trompete 1 2": maybe a copy number).
    pub level: Level,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<Note>,
}

/// What a part name says.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartText {
    pub text: String,
    pub instruments: Vec<InstrumentText>,
    pub key: Reading,
    pub voices: Voices,
    /// Words that were not understood (shown as a hint).
    pub rest: Vec<String>,
}

enum KeyWord {
    Key(&'static str),
    /// Language-dependent letter: (code in German, what it is in English).
    German(&'static str),
}

fn key_word(w: &str) -> Option<KeyWord> {
    Some(match w {
        "bb" | "b♭" => KeyWord::Key("Bb"),
        "eb" | "e♭" => KeyWord::Key("Eb"),
        "ab" | "a♭" => KeyWord::Key("Ab"),
        "db" | "d♭" => KeyWord::Key("Db"),
        "gb" | "g♭" => KeyWord::Key("Gb"),
        "f" => KeyWord::Key("F"),
        "c" => KeyWord::Key("C"),
        "a" => KeyWord::Key("A"),
        "d" => KeyWord::Key("D"),
        "g" => KeyWord::Key("G"),
        "e" => KeyWord::Key("E"),
        "b" => KeyWord::German("Bb"),
        "es" => KeyWord::German("Eb"),
        "as" => KeyWord::German("Ab"),
        "des" => KeyWord::German("Db"),
        "ges" => KeyWord::German("Gb"),
        "h" => KeyWord::German("H"),
        _ => return None,
    })
}

/// Reads a key as written in a file name, depending on the naming
/// language. Unlike [`crate::naming::parse_key`] this knows German letters,
/// but only in German batches: "B" is B♭ in German and B♮ in English.
pub fn parse_import_key(text: &str, language: Language) -> Reading {
    let words: Vec<String> = tokenize(text).iter().map(|t| fold(t)).filter(|t| t != "in").collect();
    let joined = words.join(" ");
    let flat = match joined.as_str() {
        "b flat" => Some("Bb"),
        "e flat" => Some("Eb"),
        "a flat" => Some("Ab"),
        "d flat" => Some("Db"),
        "g flat" => Some("Gb"),
        _ => None,
    };
    if let Some(code) = flat {
        return Reading { value: Some(code.into()), level: Level::Green, note: None };
    }
    match key_word(&joined) {
        Some(KeyWord::Key(code)) => Reading { value: Some(code.into()), level: Level::Green, note: None },
        Some(KeyWord::German(code)) if language == Language::De => {
            Reading { value: Some(code.into()), level: Level::Green, note: None }
        }
        Some(KeyWord::German(_)) => {
            Reading { value: None, level: Level::Yellow, note: Some(Note::AmbiguousKey(text.trim().to_owned())) }
        }
        None => Reading { value: None, level: Level::Red, note: Some(Note::UnknownKey(text.trim().to_owned())) },
    }
}

fn roman(w: &str) -> Option<&'static str> {
    Some(match w {
        "i" => "1",
        "ii" => "2",
        "iii" => "3",
        "iv" => "4",
        "v" => "5",
        "vi" => "6",
        "vii" => "7",
        "viii" => "8",
        _ => return None,
    })
}

/// (entry index, abbreviation?) of a folded alias.
fn lookup(alias: &str) -> Option<(usize, bool)> {
    DICTIONARY.iter().enumerate().find_map(|(i, e)| {
        if e.words.iter().any(|(w, _)| *w == alias) {
            Some((i, false))
        } else if e.abbreviations.contains(&alias) {
            Some((i, true))
        } else {
            None
        }
    })
}

/// A word that starts like exactly one dictionary word (≥ 4 letters:
/// "Klarin", "Tromp"), or a longer form of one ("Klarinetten").
fn fuzzy(word: &str) -> Option<usize> {
    if word.chars().count() < 4 || !word.chars().all(char::is_alphabetic) {
        return None;
    }
    let mut found: Option<usize> = None;
    for (i, e) in DICTIONARY.iter().enumerate() {
        let hit = e.words.iter().any(|(w, _)| {
            !w.contains(' ') && ((w.starts_with(word) && w.len() > word.len()) || (word.starts_with(w) && w.len() >= 5))
        });
        if hit {
            if found.is_some_and(|f| f != i) {
                return None;
            }
            found = Some(i);
        }
    }
    found
}

/// The same instrument named twice ("Klarinette Klar 1", "Trompete/Trp")
/// is listed once (a-import-instruments): at the place it was named first,
/// with the surest of its spellings (a full word before an abbreviation).
fn unique_instruments(found: Vec<InstrumentText>) -> Vec<InstrumentText> {
    let same = |a: &InstrumentText, b: &InstrumentText| match (&a.entry, &b.entry) {
        (Some(x), Some(y)) => x == y,
        (None, None) => a.alias == b.alias,
        _ => false,
    };
    let mut out: Vec<InstrumentText> = Vec::new();
    for i in found {
        match out.iter_mut().find(|o| same(o, &i)) {
            Some(o) if i.level < o.level => *o = i,
            Some(_) => {}
            None => out.push(i),
        }
    }
    out
}

/// Reads a part name. `language` decides German key letters.
pub fn parse_part(text: &str, language: Language) -> PartText {
    let tokens = tokenize(text);
    let folded: Vec<String> = tokens.iter().map(|t| fold(t)).collect();
    let mut instruments: Vec<InstrumentText> = Vec::new();
    let mut keys: Vec<(String, Reading)> = Vec::new();
    let mut voices: Vec<String> = Vec::new();
    // What came before the current word, for voices: two voices are sure
    // only with a separator directly between them.
    #[derive(PartialEq)]
    enum Prev {
        Voice,
        Separator,
        Other,
    }
    let mut prev = Prev::Other;
    let mut loose = false;
    let mut push_voice = |voices: &mut Vec<String>, before: &Prev, v: String| {
        if !voices.is_empty() && *before != Prev::Separator {
            loose = true;
        }
        voices.push(v);
    };
    let mut rest: Vec<String> = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let before = std::mem::replace(&mut prev, Prev::Other);
        // Longest dictionary match of up to three words.
        let mut matched = false;
        for n in (1..=3usize).rev() {
            if i + n > tokens.len() {
                continue;
            }
            let alias = folded[i..i + n].join(" ");
            if let Some((entry, abbreviation)) = lookup(&alias) {
                let e = &DICTIONARY[entry];
                let level = if abbreviation || e.ambiguous { Level::Yellow } else { Level::Green };
                instruments.push(InstrumentText {
                    text: tokens[i..i + n].join(" "),
                    alias,
                    entry: Some(e.name.into()),
                    level,
                });
                i += n;
                matched = true;
                break;
            }
        }
        if matched {
            continue;
        }
        let w = folded[i].as_str();
        // "in B", "in Es", "B flat"
        if w == "in" && i + 1 < tokens.len() {
            let mut n = 1;
            if folded.get(i + 2).is_some_and(|f| f == "flat") {
                n = 2;
            }
            let key_text = tokens[i + 1..=i + n].join(" ");
            let reading = parse_import_key(&key_text, language);
            if reading.level != Level::Red {
                keys.push((key_text, reading));
                i += 1 + n;
                continue;
            }
        }
        if folded.get(i + 1).is_some_and(|f| f == "flat") && key_word(w).is_some() {
            let key_text = tokens[i..=i + 1].join(" ");
            keys.push((key_text.clone(), parse_import_key(&key_text, language)));
            i += 2;
            continue;
        }
        if w.chars().all(|c| c.is_ascii_digit())
            || (w.len() >= 2
                && w[..w.len() - 1].chars().all(|c| c.is_ascii_digit())
                && w.ends_with(|c: char| c.is_ascii_lowercase()))
        {
            let v = tokens[i].trim_start_matches('0');
            push_voice(&mut voices, &before, if v.is_empty() { "0".into() } else { v.to_owned() });
            prev = Prev::Voice;
            i += 1;
            continue;
        }
        if w == "+" || w == "und" || w == "and" {
            prev = if before == Prev::Voice { Prev::Separator } else { Prev::Other };
            i += 1;
            continue;
        }
        if let Some(r) = roman(w).filter(|_| tokens[i].chars().all(|c| c.is_uppercase()) || w.len() > 1) {
            push_voice(&mut voices, &before, r.into());
            prev = Prev::Voice;
            i += 1;
            continue;
        }
        if w == "solo" {
            push_voice(&mut voices, &before, "Solo".into());
            prev = Prev::Voice;
            i += 1;
            continue;
        }
        if key_word(w).is_some() && w.len() <= 3 {
            keys.push((tokens[i].clone(), parse_import_key(&tokens[i], language)));
            i += 1;
            continue;
        }
        if FILLER.contains(&w) || w == "flat" {
            i += 1;
            continue;
        }
        if let Some(entry) = fuzzy(w) {
            instruments.push(InstrumentText {
                text: tokens[i].clone(),
                alias: w.to_owned(),
                entry: Some(DICTIONARY[entry].name.into()),
                level: Level::Yellow,
            });
            i += 1;
            continue;
        }
        rest.push(tokens[i].clone());
        i += 1;
    }
    // Unknown words become the (unknown) instrument when nothing else was found.
    if instruments.is_empty() && !rest.is_empty() {
        let text = rest.join(" ");
        instruments.push(InstrumentText { alias: alias_key(&text), text, entry: None, level: Level::Red });
        rest.clear();
    }
    let instruments = unique_instruments(instruments);
    let key = match keys.len() {
        0 => Reading::default(),
        1 => keys.pop().map(|(_, r)| r).unwrap_or_default(),
        _ => {
            let values: Vec<&str> = keys.iter().filter_map(|(_, r)| r.value.as_deref()).collect();
            if values.windows(2).all(|w| w[0] == w[1]) && values.len() == keys.len() {
                keys.swap_remove(0).1
            } else {
                let all: Vec<String> = keys.iter().map(|(t, _)| t.clone()).collect();
                Reading { value: None, level: Level::Red, note: Some(Note::TwoKeys(all.join(", "))) }
            }
        }
    };
    let values = crate::naming::clean_voices(&voices);
    let voices = if loose && values.len() > 1 {
        Voices { note: Some(Note::SeveralVoices(values.join(" "))), values, level: Level::Yellow }
    } else {
        Voices { values, level: Level::Green, note: None }
    };
    PartText { text: text.to_owned(), instruments, key, voices, rest }
}

/// German, English or undecided, from the instrument words of a batch:
/// the language with more words that exist only in it.
pub fn suggest_language<'a>(part_texts: impl IntoIterator<Item = &'a str>) -> Option<Language> {
    let (mut de, mut en) = (0usize, 0usize);
    for text in part_texts {
        let tokens: Vec<String> = tokenize(text).iter().map(|t| fold(t)).collect();
        for (i, w) in tokens.iter().enumerate() {
            for n in (1..=3usize).rev() {
                if i + n > tokens.len() {
                    continue;
                }
                let alias = tokens[i..i + n].join(" ");
                let lang = DICTIONARY.iter().find_map(|e| e.words.iter().find(|(x, _)| *x == alias).map(|(_, l)| *l));
                match lang {
                    Some(Some(Language::De)) => de += 1,
                    Some(Some(Language::En)) => en += 1,
                    _ => {}
                }
                if lang.is_some() {
                    break;
                }
            }
            if matches!(w.as_str(), "es" | "as" | "des" | "ges") {
                de += 1;
            }
        }
    }
    match de.cmp(&en) {
        std::cmp::Ordering::Greater => Some(Language::De),
        std::cmp::Ordering::Less => Some(Language::En),
        std::cmp::Ordering::Equal => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(text: &str, lang: Language) -> (Vec<String>, Option<String>, Vec<String>) {
        let p = parse_part(text, lang);
        (
            p.instruments.iter().map(|i| i.entry.clone().unwrap_or_else(|| format!("?{}", i.text))).collect(),
            p.key.value,
            p.voices.values,
        )
    }

    fn s(v: &str) -> Option<String> {
        Some(v.into())
    }

    /// Voices: "1+2" ⇒ ["1", "2"].
    fn v(list: &str) -> Vec<String> {
        list.split('+').filter(|x| !x.is_empty()).map(str::to_owned).collect()
    }

    #[test]
    fn tokens() {
        assert_eq!(tokenize("KlarinetteEb"), ["Klarinette", "Eb"]);
        assert_eq!(tokenize("Klar2B"), ["Klar", "2", "B"]);
        assert_eq!(tokenize("BbClarinet II"), ["Bb", "Clarinet", "II"]);
        assert_eq!(tokenize("Trompete 1+2"), ["Trompete", "1", "+", "2"]);
        assert_eq!(tokenize("Horn 1/2, 3"), ["Horn", "1", "+", "2", "+", "3"]);
        assert_eq!(tokenize("Horn3a"), ["Horn", "3a"]);
        assert_eq!(tokenize("1st Trumpet"), ["1", "st", "Trumpet"]);
        assert_eq!(tokenize("Pos. 3"), ["Pos", "3"]);
        assert_eq!(tokenize("Es-Klarinette"), ["Es", "Klarinette"]);
    }

    #[test]
    fn german_and_english_names() {
        use Language::*;
        assert_eq!(parsed("Klarinette 2 in B", De), (vec!["Clarinet".into()], s("Bb"), v("2")));
        assert_eq!(parsed("Bb Clarinet II", En), (vec!["Clarinet".into()], s("Bb"), v("2")));
        assert_eq!(parsed("Klar2B", De), (vec!["Clarinet".into()], s("Bb"), v("2")));
        assert_eq!(parsed("KlarinetteEb", De), (vec!["Clarinet".into()], s("Eb"), v("")));
        assert_eq!(parsed("KlarinetteEb", En), (vec!["Clarinet".into()], s("Eb"), v("")), "Eb is the same in both");
        assert_eq!(parsed("Trompete2", De), (vec!["Trumpet".into()], None, v("2")));
        assert_eq!(parsed("Trp 1", En), (vec!["Trumpet".into()], None, v("1")));
        assert_eq!(parsed("Pos. 3", De), (vec!["Trombone".into()], None, v("3")));
        assert_eq!(parsed("Trompete 1+2", De), (vec!["Trumpet".into()], None, v("1+2")));
        assert_eq!(parsed("Horn in F 1", En), (vec!["Horn".into()], s("F"), v("1")));
        assert_eq!(parsed("Horn 3a", En), (vec!["Horn".into()], None, v("3a")));
        assert_eq!(parsed("Es-Klarinette", De), (vec!["Clarinet".into()], s("Eb"), v("")));
        assert_eq!(parsed("Tenorsaxophon", De), (vec!["Tenor Saxophone".into()], None, v("")));
        assert_eq!(parsed("TenorSax 2", En), (vec!["Tenor Saxophone".into()], None, v("2")));
        assert_eq!(parsed("Bass Clarinet in B flat", En), (vec!["Bass Clarinet".into()], s("Bb"), v("")));
        assert_eq!(parsed("Flöte/Piccolo", De), (vec!["Flute".into(), "Piccolo".into()], None, v("")));
        assert_eq!(parsed("Flügelhorn 1 in B", De), (vec!["Flugelhorn".into()], s("Bb"), v("1")));
        assert_eq!(parsed("2nd Trumpet in Bb", En), (vec!["Trumpet".into()], s("Bb"), v("2")));
        assert_eq!(parsed("Tuba", De), (vec!["Tuba".into()], None, v("")));
        assert_eq!(parsed("Stimme Tenorhorn 1", De), (vec!["Tenor Horn".into()], None, v("1")));
    }

    #[test]
    fn no_key_is_ever_assumed() {
        for (text, lang) in [
            ("Klarinette 1", Language::De),
            ("Clarinet 1", Language::En),
            ("Trompete", Language::De),
            ("Horn 2", Language::En),
        ] {
            let p = parse_part(text, lang);
            assert_eq!(p.key, Reading::default(), "{text}");
        }
    }

    #[test]
    fn german_letters_only_in_german_batches() {
        let de = parse_part("Klarinette 1 in B", Language::De);
        assert_eq!((de.key.value.as_deref(), de.key.level), (Some("Bb"), Level::Green));
        let en = parse_part("Clarinet 1 in B", Language::En);
        assert_eq!((en.key.value.as_deref(), en.key.level), (None, Level::Yellow), "ambiguous, not guessed");
        assert_eq!(en.key.note, Some(Note::AmbiguousKey("B".into())));
        assert_eq!(parse_part("Horn in Es", Language::De).key.value.as_deref(), Some("Eb"));
        assert_eq!(parse_part("Horn in Es", Language::En).key.value, None);
        assert_eq!(parse_part("Bass in H", Language::De).key.value.as_deref(), Some("H"));
        assert_eq!(parse_import_key("in As", Language::De).value.as_deref(), Some("Ab"));
        assert_eq!(parse_import_key("B-flat", Language::En).value.as_deref(), Some("Bb"));
        assert_eq!(parse_import_key("B", Language::En).level, Level::Yellow);
        assert_eq!(parse_import_key("x", Language::En).level, Level::Red);
        // Two different keys contradict each other.
        let two = parse_part("Horn F Es", Language::De);
        assert_eq!((two.key.value, two.key.level), (None, Level::Red));
    }

    #[test]
    fn partitur_and_direktion_are_different_instruments() {
        let entry = |t: &str, l| parse_part(t, l).instruments[0].entry.clone().unwrap();
        for t in ["Partitur", "Score", "Full score", "Full Score"] {
            assert_eq!(entry(t, Language::En), "Partitur", "{t}");
        }
        for t in ["Direktion", "Direction", "Dirigierstimme", "Conductor", "Conductor score", "Condensed score"] {
            assert_eq!(entry(t, Language::En), "Direktion", "{t}");
        }
        let p = parse_part("Conductor Score", Language::En);
        assert_eq!(p.instruments.len(), 1, "one instrument, not Direktion + Partitur");
    }

    #[test]
    fn levels_of_instrument_words() {
        let level = |t: &str| parse_part(t, Language::De).instruments[0].level;
        assert_eq!(level("Klarinette"), Level::Green);
        assert_eq!(level("Klar"), Level::Yellow, "abbreviation");
        assert_eq!(level("Klarin"), Level::Yellow, "word start");
        assert_eq!(level("Klarinetten"), Level::Yellow, "longer form");
        assert_eq!(level("Bass"), Level::Yellow, "ambiguous word");
        let unknown = parse_part("Alphorn 1", Language::De);
        assert_eq!(
            unknown.instruments[0],
            InstrumentText { text: "Alphorn".into(), alias: "alphorn".into(), entry: None, level: Level::Red }
        );
        assert_eq!(unknown.voices.values, ["1"]);
        assert_eq!(alias_key(" Bass  Klar. "), "bass klar");
        assert_eq!(parse_part("Bass Alphorn", Language::De).instruments[0].alias, "bass", "dictionary words first");
        let rest = parse_part("Trompete 1 Zusatz", Language::De);
        assert_eq!(rest.rest, ["Zusatz"]);
    }

    #[test]
    fn several_voices() {
        use Language::*;
        // An explicit separator: sure.
        for text in [
            "Trompete 1+2",
            "Trompete 1 + 2",
            "Trompete 1/2",
            "Trompete 1 & 2",
            "Trompete I+II",
            "Trompete 1 und 2",
            "Trompete 1,2",
        ] {
            let p = parse_part(text, De);
            assert_eq!(
                (p.voices.values.clone(), p.voices.level, p.voices.note),
                (v("1+2"), Level::Green, None),
                "{text}"
            );
            assert_eq!(p.instruments.len(), 1, "{text}");
        }
        assert_eq!(parsed("Flügelhorn I & II in B", De), (vec!["Flugelhorn".into()], s("Bb"), v("1+2")));
        assert_eq!(parsed("Horn 1/2/3 in F", En), (vec!["Horn".into()], s("F"), v("1+2+3")));
        assert_eq!(parsed("Trompete 2+1", De).2, v("2+1"), "order as written");
        assert_eq!(parsed("Trompete 1+1", De).2, v("1"), "no duplicates");
        // Numbers next to each other: read as voices, but flagged (maybe a copy number).
        let loose = parse_part("Trompete 1 2", De);
        assert_eq!((loose.voices.values.clone(), loose.voices.level), (v("1+2"), Level::Yellow));
        assert_eq!(loose.voices.note, Some(Note::SeveralVoices("1 2".into())));
        let apart = parse_part("Klarinette 1 in B 2", De);
        assert_eq!((apart.voices.values, apart.voices.level), (v("1+2"), Level::Yellow), "not joined by a separator");
        // Instruments joined by "/" stay instruments.
        assert_eq!(parsed("Flöte/Piccolo 1", De), (vec!["Flute".into(), "Piccolo".into()], None, v("1")));
    }

    #[test]
    fn language_suggestion() {
        assert_eq!(suggest_language(["Klarinette 1", "Trompete 2", "Tuba"]), Some(Language::De));
        assert_eq!(suggest_language(["Clarinet 1", "Trumpet 2", "Tuba"]), Some(Language::En));
        assert_eq!(suggest_language(["Tuba", "Horn"]), None);
        assert_eq!(suggest_language(["Horn in Es"]), Some(Language::De));
    }

    #[test]
    fn an_instrument_named_twice_is_listed_once() {
        let names =
            |t: &str| parse_part(t, De).instruments.iter().map(|i| (i.text.clone(), i.level)).collect::<Vec<_>>();
        // Full word and abbreviation: once, the full word, at the first place.
        assert_eq!(names("Klarinette Klar 1"), [("Klarinette".to_owned(), Level::Green)]);
        assert_eq!(names("Trp/Trompete 1"), [("Trompete".to_owned(), Level::Green)]);
        // German and English word, and the same word twice.
        assert_eq!(names("Trompete Trumpet"), [("Trompete".to_owned(), Level::Green)]);
        assert_eq!(names("Horn 1 + Horn 2"), [("Horn".to_owned(), Level::Green)]);
        assert_eq!(parse_part("Horn 1 + Horn 2", De).voices.values, ["1", "2"]);
        // Different instruments keep their order.
        let both = parse_part("Flöte Piccolo Flöte", De);
        let entries: Vec<_> = both.instruments.iter().map(|i| i.entry.clone().unwrap()).collect();
        assert_eq!(entries, ["Flute", "Piccolo"]);
    }
}
