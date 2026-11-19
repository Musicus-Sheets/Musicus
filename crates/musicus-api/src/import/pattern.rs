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
//! | `{*}` | any text without `/`, ignored (e.g. a counter `Stimme06`); as short as possible |
//! | `[ … ]` | optional; the pattern with the most optional groups present wins |
//! | `/` | folder separator; everything else is literal text, compared ignoring case |
//!
//! A trailing `.pdf` of the pattern and of the path is ignored. Paths are
//! relative to the folder the librarian selected (without its name).
//! In values, `_` counts as a space and spaces are collapsed.

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

/// Why a pattern cannot be used (shown under the pattern field).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PatternError {
    UnknownPlaceholder(String),
    Unbalanced,
    MissingPart,
    MissingTitleOrNumber,
    Twice(&'static str),
    TooManyOptionals,
}

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
            Self::TooManyOptionals => f.write_str("at most 6 optional groups [ ]"),
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

/// `_` as space, whitespace collapsed, trimmed.
pub fn clean(value: &str) -> String {
    value.replace('_', " ").split_whitespace().collect::<Vec<_>>().join(" ")
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
                    next.push((atoms.clone(), *n));
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
            return Err(PatternError::Twice(match p {
                Placeholder::Number => "number",
                Placeholder::Title => "title",
                Placeholder::Composer => "composer",
                Placeholder::Arranger => "arranger",
                Placeholder::Genre => "genre",
                _ => "part",
            }));
        }
        if optionals > 6 {
            return Err(PatternError::TooManyOptionals);
        }
        if !seen.contains(&Placeholder::Part) {
            return Err(PatternError::MissingPart);
        }
        if !seen.contains(&Placeholder::Title) && !seen.contains(&Placeholder::Number) {
            return Err(PatternError::MissingTitleOrNumber);
        }
        let mut alternatives = expand(&tokens);
        // Stable: among equally many groups, the order of the pattern.
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
            let ok = s.len() >= i + lit.len()
                && lit.iter().zip(&s[i..]).all(|(a, b)| a == b || a.to_lowercase().eq(b.to_lowercase()));
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

/// The presets, the default first. Detection tries every one.
pub const PRESETS: &[Preset] = &[
    Preset {
        id: "folder",
        name: "Folder per piece",
        pattern: "[{genre}/][{number} ]{title}[ - {composer}]/{part}.pdf",
        example: "Marches/0304 Alte Kameraden - Teike/Klarinette 1 in B.pdf",
    },
    Preset {
        id: "category",
        name: "Category / piece",
        pattern: "{genre}/[{number} ]{title}[ - {composer}]/{part}.pdf",
        example: "Marches/Alte Kameraden/Trompete 1+2.pdf",
    },
    Preset {
        id: "flat-title",
        name: "One folder, title in the file name",
        pattern: "{title} - {part}.pdf",
        example: "Alte Kameraden - Horn in F 1.pdf",
    },
    Preset {
        id: "flat-number",
        name: "One folder, archive number in the file name",
        pattern: "{number}Stimme{*}_{part}.pdf",
        example: "304Stimme06_Klarinette2.pdf",
    },
];

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
        let p = PRESETS[0].pattern;
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
        // Too deep or too flat.
        assert_eq!(m(p, "a/b/c/d.pdf"), None);
        assert_eq!(m(p, "Partitur.pdf"), None);
    }

    #[test]
    fn flat_number_preset_ignores_counters() {
        let p = PRESETS[3].pattern;
        assert_eq!(m(p, "304Stimme06_Klarinette2.pdf"), Some(fields(Some("304"), None, None, "Klarinette2")));
        assert_eq!(m(p, "28Stimme04_KlarinetteEb.pdf"), Some(fields(Some("28"), None, None, "KlarinetteEb")));
        assert_eq!(m(p, "302Stimme-06_Klarinette2.pdf"), Some(fields(Some("302"), None, None, "Klarinette2")));
        // An underscore inside the part name stays in the part (ignored text is as short as possible).
        assert_eq!(m(p, "12Stimme01_Trompete_1.pdf"), Some(fields(Some("12"), None, None, "Trompete 1")));
        assert_eq!(m(p, "Alte Kameraden - Horn.pdf"), None);
    }

    #[test]
    fn flat_title_and_category() {
        assert_eq!(
            m(PRESETS[2].pattern, "Alte Kameraden - Horn in F 1.pdf"),
            Some(fields(None, Some("Alte Kameraden"), None, "Horn in F 1"))
        );
        assert_eq!(m(PRESETS[1].pattern, "Alte Kameraden/Horn.pdf"), None, "category is required");
        assert_eq!(m(PRESETS[1].pattern, "Polkas/Böhmischer Traum/Horn.pdf").unwrap().genre.as_deref(), Some("Polkas"));
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
    }

    #[test]
    fn errors() {
        assert_eq!(Pattern::parse("{title}/{foo}.pdf"), Err(PatternError::UnknownPlaceholder("foo".into())));
        assert_eq!(Pattern::parse("[{title}/{part}"), Err(PatternError::Unbalanced));
        assert_eq!(Pattern::parse("{title}"), Err(PatternError::MissingPart));
        assert_eq!(Pattern::parse("{composer}/{part}"), Err(PatternError::MissingTitleOrNumber));
        assert_eq!(Pattern::parse("{title}/{title} {part}"), Err(PatternError::Twice("title")));
        assert!(Pattern::parse("{number}/{part}").is_ok());
        for p in PRESETS {
            let pattern = Pattern::parse(p.pattern).unwrap();
            assert!(pattern.matches(p.example).is_some(), "{} matches its example", p.id);
        }
    }
}
