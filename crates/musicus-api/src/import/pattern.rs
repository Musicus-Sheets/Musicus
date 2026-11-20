//! File name patterns of the mass import: which part of a path is the
//! archive number, the title, the composer, … and which is the part name.
//!
//! Syntax (a-import-patterns):
//!
//! | Element | Meaning |
//! | --- | --- |
//! | `{number}` | archive number: no spaces, at least one digit (`0304`, `A-12`); as short as possible |
//! | `{title}`, `{composer}`, `{arranger}`, `{genre}` | text without `/`; as long as possible |
//! | `{part}` | the part name (instrument, key, voices), parsed by [`super::parts`] |
//! | `{*}` | any text without `/`, ignored (e.g. a counter `Part06`); as short as possible |
//! | `[ … ]` | optional; the reading with the most optional groups present wins, on a tie the one with the earlier groups |
//! | `/` | folder separator; everything else is literal text, compared ignoring case |
//!
//! A trailing `.pdf` of the pattern and of the path is ignored. Paths are
//! relative to the folder the librarian selected (without its name).
//!
//! Separators are tolerant (a-import-patterns): `_` counts as a space, so a
//! space in the pattern also matches `_` in the path, and values are cleaned
//! (`_` ⇒ space, spaces collapsed, leading and trailing `-`, `–`, `_` and
//! spaces dropped). One pattern `{number} {part}` therefore reads
//! `0304 Horn`, `0304_Horn`, `0304 - Horn` and `0304_-_Horn` alike.

use serde::{Deserialize, Serialize};

/// A placeholder of a pattern.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placeholder {
    Number,
    Title,
    Composer,
    Arranger,
    Genre,
    Part,
    /// `{*}`: matched text is dropped.
    Ignore,
}

impl Placeholder {
    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "number" => Self::Number,
            "title" => Self::Title,
            "composer" => Self::Composer,
            "arranger" => Self::Arranger,
            "genre" => Self::Genre,
            "part" => Self::Part,
            "*" => Self::Ignore,
            _ => return None,
        })
    }

    /// Shortest match first (number, ignored text) or longest first.
    fn lazy(self) -> bool {
        matches!(self, Self::Number | Self::Ignore)
    }

    fn accepts(self, value: &str) -> bool {
        match self {
            Self::Ignore => true,
            Self::Number => !value.chars().any(char::is_whitespace) && value.chars().any(|c| c.is_ascii_digit()),
            _ => !clean(value).is_empty(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Lit(String),
    Field(Placeholder),
    Optional(Vec<Token>),
}

#[derive(Clone, Debug, PartialEq)]
enum Atom {
    Lit(Vec<char>),
    Field(Placeholder),
}

/// A parsed pattern, expanded into its alternatives (every combination of
/// optional groups), most groups present first.
#[derive(Clone, Debug, PartialEq)]
pub struct Pattern {
    source: String,
    alternatives: Vec<Vec<Atom>>,
}

/// Why a pattern cannot be used (shown under the pattern field; the client
/// renders the text). JSON: `{"kind": snake_case, "detail": …}`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
pub enum PatternError {
    UnknownPlaceholder(String),
    Unbalanced,
    MissingPart,
    MissingTitleOrNumber,
    /// The placeholder name (`title`, …) that appears more than once.
    Twice(String),
    /// More than [`MAX_OPTIONALS`] groups.
    TooManyOptionals,
}

/// Upper limit of optional groups `[…]` in a pattern.
pub const MAX_OPTIONALS: usize = 6;

impl std::fmt::Display for PatternError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownPlaceholder(p) => write!(
                f,
                "unknown placeholder {{{p}}}; use {{number}}, {{title}}, {{composer}}, {{arranger}}, {{genre}}, {{part}} or {{*}}"
            ),
            Self::Unbalanced => f.write_str("brackets [ ] or braces { } do not match"),
            Self::MissingPart => f.write_str("the pattern needs {part}"),
            Self::MissingTitleOrNumber => f.write_str("the pattern needs {title} or {number}"),
            Self::Twice(p) => write!(f, "{{{p}}} may appear only once"),
            Self::TooManyOptionals => write!(f, "at most {MAX_OPTIONALS} optional groups [ ]"),
        }
    }
}

/// What a path gave for each placeholder (cleaned; `None` = not in the
/// pattern or in an optional group that was absent).
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PathFields {
    pub number: Option<String>,
    pub title: Option<String>,
    pub composer: Option<String>,
    pub arranger: Option<String>,
    pub genre: Option<String>,
    /// The part name text (`{part}`), always present in a match.
    pub part: String,
}

/// `_` as space, whitespace collapsed, separators (`-`, `–`, spaces) at
/// both ends dropped.
pub fn clean(value: &str) -> String {
    let spaced = value.replace('_', " ");
    let trimmed = spaced.trim_matches(|c: char| c.is_whitespace() || c == '-' || c == '–');
    trimmed.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A pattern character matches a path character: equal ignoring case, and a
/// space of the pattern also matches `_`.
fn same_char(pattern: char, path: char) -> bool {
    pattern == path || (pattern == ' ' && path == '_') || pattern.to_lowercase().eq(path.to_lowercase())
}

fn strip_pdf(s: &str) -> &str {
    let n = s.len();
    if n >= 4 && s.is_char_boundary(n - 4) && s[n - 4..].eq_ignore_ascii_case(".pdf") { &s[..n - 4] } else { s }
}

fn parse_tokens(chars: &mut std::iter::Peekable<std::str::Chars>, nested: bool) -> Result<Vec<Token>, PatternError> {
    let mut tokens = Vec::new();
    let mut lit = String::new();
    while let Some(c) = chars.next() {
        match c {
            '{' => {
                let mut name = String::new();
                loop {
                    match chars.next() {
                        Some('}') => break,
                        Some(c) if c != '{' && c != '[' && c != ']' => name.push(c),
                        _ => return Err(PatternError::Unbalanced),
                    }
                }
                let name = name.trim().to_lowercase();
                let p = Placeholder::parse(&name).ok_or(PatternError::UnknownPlaceholder(name))?;
                if !lit.is_empty() {
                    tokens.push(Token::Lit(std::mem::take(&mut lit)));
                }
                tokens.push(Token::Field(p));
            }
            '[' => {
                if !lit.is_empty() {
                    tokens.push(Token::Lit(std::mem::take(&mut lit)));
                }
                tokens.push(Token::Optional(parse_tokens(chars, true)?));
            }
            ']' if nested => {
                if !lit.is_empty() {
                    tokens.push(Token::Lit(lit));
                }
                return Ok(tokens);
            }
            ']' | '}' => return Err(PatternError::Unbalanced),
            c => lit.push(c),
        }
    }
    if nested {
        return Err(PatternError::Unbalanced);
    }
    if !lit.is_empty() {
        tokens.push(Token::Lit(lit));
    }
    Ok(tokens)
}

fn count(tokens: &[Token], f: &mut impl FnMut(&Token)) {
    for t in tokens {
        f(t);
        if let Token::Optional(inner) = t {
            count(inner, f);
        }
    }
}

/// Every combination of optional groups: (atoms, groups present).
fn expand(tokens: &[Token]) -> Vec<(Vec<Atom>, usize)> {
    let mut out: Vec<(Vec<Atom>, usize)> = vec![(Vec::new(), 0)];
    for t in tokens {
        match t {
            Token::Lit(l) => out.iter_mut().for_each(|(a, _)| push_lit(a, l)),
            Token::Field(p) => out.iter_mut().for_each(|(a, _)| a.push(Atom::Field(*p))),
            Token::Optional(inner) => {
                let options = expand(inner);
                let mut next = Vec::new();
                for (atoms, n) in &out {
                    // With the group first: among equally many groups, the
                    // earlier group of the pattern wins ("0304 - Title" is
                    // number and title, not title and composer).
                    for (opt, m) in &options {
                        let mut a = atoms.clone();
                        for atom in opt {
                            match atom {
                                Atom::Lit(l) => push_lit(&mut a, &l.iter().collect::<String>()),
                                f => a.push(f.clone()),
                            }
                        }
                        next.push((a, n + m + 1));
                    }
                    next.push((atoms.clone(), *n));
                }
                out = next;
            }
        }
    }
    out
}

fn push_lit(atoms: &mut Vec<Atom>, lit: &str) {
    if let Some(Atom::Lit(prev)) = atoms.last_mut() {
        prev.extend(lit.chars());
    } else {
        atoms.push(Atom::Lit(lit.chars().collect()));
    }
}

impl Pattern {
    pub fn parse(source: &str) -> Result<Self, PatternError> {
        let trimmed = strip_pdf(source.trim());
        let tokens = parse_tokens(&mut trimmed.chars().peekable(), false)?;
        let (mut seen, mut optionals) = (Vec::new(), 0);
        let mut twice = None;
        count(&tokens, &mut |t| match t {
            Token::Field(p) if *p != Placeholder::Ignore => {
                if seen.contains(p) {
                    twice = Some(*p);
                }
                seen.push(*p);
            }
            Token::Optional(_) => optionals += 1,
            _ => {}
        });
        if let Some(p) = twice {
            return Err(PatternError::Twice(
                match p {
                    Placeholder::Number => "number",
                    Placeholder::Title => "title",
                    Placeholder::Composer => "composer",
                    Placeholder::Arranger => "arranger",
                    Placeholder::Genre => "genre",
                    _ => "part",
                }
                .into(),
            ));
        }
        if optionals > MAX_OPTIONALS {
            return Err(PatternError::TooManyOptionals);
        }
        if !seen.contains(&Placeholder::Part) {
            return Err(PatternError::MissingPart);
        }
        if !seen.contains(&Placeholder::Title) && !seen.contains(&Placeholder::Number) {
            return Err(PatternError::MissingTitleOrNumber);
        }
        let mut alternatives = expand(&tokens);
        // Stable: among equally many groups, the earlier groups present first.
        alternatives.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
        Ok(Self { source: source.trim().to_owned(), alternatives: alternatives.into_iter().map(|(a, _)| a).collect() })
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    /// The fields of a path, or `None` if no alternative matches it completely.
    pub fn matches(&self, path: &str) -> Option<PathFields> {
        let path: Vec<char> = strip_pdf(path.trim_matches('/')).chars().collect();
        for atoms in &self.alternatives {
            let mut found = Vec::new();
            if match_atoms(atoms, &path, 0, &mut found) {
                let mut f = PathFields::default();
                for (p, v) in found {
                    let v = clean(&v);
                    match p {
                        Placeholder::Number => f.number = Some(v),
                        Placeholder::Title => f.title = Some(v),
                        Placeholder::Composer => f.composer = Some(v),
                        Placeholder::Arranger => f.arranger = Some(v),
                        Placeholder::Genre => f.genre = Some(v),
                        Placeholder::Part => f.part = v,
                        Placeholder::Ignore => {}
                    }
                }
                return Some(f);
            }
        }
        None
    }
}

fn match_atoms(atoms: &[Atom], s: &[char], i: usize, out: &mut Vec<(Placeholder, String)>) -> bool {
    let Some(first) = atoms.first() else { return i == s.len() };
    match first {
        Atom::Lit(lit) => {
            let ok = s.len() >= i + lit.len() && lit.iter().zip(&s[i..]).all(|(a, b)| same_char(*a, *b));
            ok && match_atoms(&atoms[1..], s, i + lit.len(), out)
        }
        Atom::Field(p) => {
            let end = s[i..].iter().position(|c| *c == '/').map_or(s.len(), |k| i + k);
            let min = if *p == Placeholder::Ignore { i } else { i + 1 };
            if min > end {
                return false;
            }
            let ends: Box<dyn Iterator<Item = usize>> =
                if p.lazy() { Box::new(min..=end) } else { Box::new((min..=end).rev()) };
            for j in ends {
                let value: String = s[i..j].iter().collect();
                if !p.accepts(&value) {
                    continue;
                }
                out.push((*p, value));
                if match_atoms(&atoms[1..], s, j, out) {
                    return true;
                }
                out.pop();
            }
            false
        }
    }
}

/// A ready-made pattern the librarian can choose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Preset {
    pub id: &'static str,
    pub name: &'static str,
    pub pattern: &'static str,
    /// Example path for the help text.
    pub example: &'static str,
}

/// The presets, the default first. Detection tries every one
/// ([`super::detect`]); among presets that fit equally well the earlier one
/// wins, so the more specific reading comes first. The presets describe
/// common archive layouts in general terms; anything special (a counter in
/// the file name, another order) is a pattern of the librarian's own.
pub const PRESETS: &[Preset] = &[
    Preset {
        id: "folder",
        name: "Folder per piece",
        pattern: "[{genre}/][{number} ]{title}[ - {composer}]/{part}.pdf",
        example: "Marches/0304 Radetzky March - Strauss/Clarinet 1 in Bb.pdf",
    },
    Preset {
        id: "flat-number",
        name: "Number and part in the file name",
        pattern: "{number} {part}.pdf",
        example: "0304 - Trumpet 1.pdf",
    },
    Preset {
        id: "flat-title",
        name: "Title and part in the file name",
        pattern: "[{number} ]{title} - {part}.pdf",
        example: "0304 Radetzky March - Horn in F 1.pdf",
    },
];

/// The preset with this id.
pub fn preset(id: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| p.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(pattern: &str, path: &str) -> Option<PathFields> {
        Pattern::parse(pattern).unwrap().matches(path)
    }

    fn fields(number: Option<&str>, title: Option<&str>, composer: Option<&str>, part: &str) -> PathFields {
        PathFields {
            number: number.map(Into::into),
            title: title.map(Into::into),
            composer: composer.map(Into::into),
            part: part.into(),
            ..Default::default()
        }
    }

    #[test]
    fn folder_preset() {
        let p = preset("folder").unwrap().pattern;
        assert_eq!(
            m(p, "Marches/0304 Alte Kameraden - Teike/Klarinette 1 in B.pdf"),
            Some(PathFields {
                genre: Some("Marches".into()),
                ..fields(Some("0304"), Some("Alte Kameraden"), Some("Teike"), "Klarinette 1 in B")
            })
        );
        // Without category, number or composer.
        assert_eq!(m(p, "Alte Kameraden/Partitur.PDF"), Some(fields(None, Some("Alte Kameraden"), None, "Partitur")));
        assert_eq!(
            m(p, "0304 Alte Kameraden/Trompete 1+2.pdf"),
            Some(fields(Some("0304"), Some("Alte Kameraden"), None, "Trompete 1+2"))
        );
        // A title word without digit is not taken as number; the last " - " separates the composer.
        assert_eq!(
            m(p, "Marsch - Teil 2 - Fučík/Horn_in_F__1.pdf"),
            Some(fields(None, Some("Marsch - Teil 2"), Some("Fučík"), "Horn in F 1"))
        );
        assert_eq!(m(p, "A-12 Polka/Tuba.pdf").unwrap().number.as_deref(), Some("A-12"));
        // Separators: "number - title" and underscores.
        assert_eq!(
            m(p, "0304 - Alte Kameraden/Tuba.pdf"),
            Some(fields(Some("0304"), Some("Alte Kameraden"), None, "Tuba"))
        );
        assert_eq!(
            m(p, "0304_Alte_Kameraden_-_Teike/Tuba.pdf"),
            Some(fields(Some("0304"), Some("Alte Kameraden"), Some("Teike"), "Tuba"))
        );
        // Too deep or too flat.
        assert_eq!(m(p, "a/b/c/d.pdf"), None);
        assert_eq!(m(p, "Partitur.pdf"), None);
    }

    #[test]
    fn flat_number_preset_accepts_common_separators() {
        let p = preset("flat-number").unwrap().pattern;
        let horn = |n: &str| Some(fields(Some(n), None, None, "Horn in F 1"));
        assert_eq!(m(p, "0304 Horn in F 1.pdf"), horn("0304"));
        assert_eq!(m(p, "0304_Horn_in_F_1.pdf"), horn("0304"));
        assert_eq!(m(p, "0304 - Horn in F 1.pdf"), horn("0304"));
        assert_eq!(m(p, "0304_-_Horn in F 1.pdf"), horn("0304"));
        assert_eq!(m(p, "A-12 Horn in F 1.pdf"), horn("A-12"));
        // The number is the first word with a digit; a title word is not a number.
        assert_eq!(m(p, "Radetzky March - Horn.pdf"), None);
        assert_eq!(m(p, "0304.pdf"), None, "no part");
        assert_eq!(m(p, "Marches/0304 Horn.pdf"), None, "one folder only");
    }

    #[test]
    fn flat_title_preset_with_and_without_number() {
        let p = preset("flat-title").unwrap().pattern;
        assert_eq!(
            m(p, "Radetzky March - Horn in F 1.pdf"),
            Some(fields(None, Some("Radetzky March"), None, "Horn in F 1"))
        );
        assert_eq!(
            m(p, "0304 Radetzky March - Tuba.pdf"),
            Some(fields(Some("0304"), Some("Radetzky March"), None, "Tuba"))
        );
        // "number - title - part" and underscores instead of spaces.
        assert_eq!(
            m(p, "0304 - Radetzky March - Tuba.pdf"),
            Some(fields(Some("0304"), Some("Radetzky March"), None, "Tuba"))
        );
        assert_eq!(
            m(p, "0304_Radetzky_March_-_Tuba.pdf"),
            Some(fields(Some("0304"), Some("Radetzky March"), None, "Tuba"))
        );
        // The last " - " separates the part.
        assert_eq!(m(p, "Suite - Part 2 - Oboe.pdf"), Some(fields(None, Some("Suite - Part 2"), None, "Oboe")));
        assert_eq!(m(p, "Oboe.pdf"), None);
    }

    #[test]
    fn values_are_cleaned_of_separators() {
        assert_eq!(clean(" - Radetzky_March  –"), "Radetzky March");
        assert_eq!(clean("Horn_in_F__1"), "Horn in F 1");
        assert_eq!(clean("Suite - Part 2"), "Suite - Part 2");
    }

    #[test]
    fn custom_patterns() {
        let f = m("{genre}/{number}_{title}_{part}", "Konzert/0007_Ouvertüre_Flöte 1.pdf").unwrap();
        assert_eq!(
            (f.genre.as_deref(), f.number.as_deref(), f.part.as_str()),
            (Some("Konzert"), Some("0007"), "Flöte 1")
        );
        assert_eq!(f.title.as_deref(), Some("Ouvertüre"));
        let f = m("{title} ({arranger})/{*}-{part}.pdf", "Moment for Morricone (Johan de Meij)/03-Oboe.pdf").unwrap();
        assert_eq!((f.arranger.as_deref(), f.part.as_str()), (Some("Johan de Meij"), "Oboe"));
        // A counter between number and part, written the librarian's own way.
        let f = m("{number}Part{*}_{part}.pdf", "0304Part06_Clarinet 2.pdf").unwrap();
        assert_eq!((f.number.as_deref(), f.part.as_str()), (Some("0304"), "Clarinet 2"));
        // A literal "_" matches only "_"; underscores inside the part stay in it.
        let f = m("{number}Part{*}_{part}.pdf", "12part-01_Trumpet_1.pdf").unwrap();
        assert_eq!((f.number.as_deref(), f.part.as_str()), (Some("12"), "Trumpet 1"));
        assert_eq!(m("{number}Part{*}_{part}.pdf", "12 Trumpet 1.pdf"), None);
    }

    #[test]
    fn errors() {
        assert_eq!(Pattern::parse("{title}/{foo}.pdf"), Err(PatternError::UnknownPlaceholder("foo".into())));
        assert_eq!(Pattern::parse("[{title}/{part}"), Err(PatternError::Unbalanced));
        assert_eq!(Pattern::parse("{title}"), Err(PatternError::MissingPart));
        assert_eq!(Pattern::parse("{composer}/{part}"), Err(PatternError::MissingTitleOrNumber));
        assert_eq!(Pattern::parse("{title}/{title} {part}"), Err(PatternError::Twice("title".into())));
        assert!(Pattern::parse("{number}/{part}").is_ok());
        for p in PRESETS {
            let pattern = Pattern::parse(p.pattern).unwrap();
            assert!(pattern.matches(p.example).is_some(), "{} matches its example", p.id);
        }
    }
}
