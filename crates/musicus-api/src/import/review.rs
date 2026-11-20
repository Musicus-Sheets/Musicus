//! From file paths to proposed pieces ([`propose`]) and from a stored
//! proposal plus the librarian's changes to what the review shows
//! ([`review_piece`]): resolved instruments, colours, notes, a suggested
//! existing piece, the groups that will be able to read each part.
//!
//! The proposal (parsed text, candidates with their source) is stored by the
//! server when the file list arrives; resolution runs on every read against
//! the current library, so a new instrument or alias turns rows green at once.

use super::{
    Language, Level, Note, Source,
    parts::{self, DICTIONARY, PartText, fold, parse_part},
    pattern::{PathFields, Pattern},
};
use crate::{
    dto::{Group, Id, Instrument},
    layout::{PageLayout, RegionKind},
    naming,
};
use serde::{Deserialize, Serialize};

/// One value a source offers for a field.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub value: String,
    pub source: Source,
}

/// What the file names say about a piece (stored as JSON, `import_items.proposal`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
pub struct PieceGuess {
    /// Grouping key: `n:<archive key>` or `t:<title>|<composer>` (folded).
    pub key: String,
    pub number: Vec<Candidate>,
    pub title: Vec<Candidate>,
    pub composer: Vec<Candidate>,
    pub arranger: Vec<Candidate>,
    pub genre: Vec<Candidate>,
}

/// What the file name says about a part (stored as JSON).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct PartGuess {
    pub path: String,
    pub text: PartText,
}

/// A proposed piece with its parts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Proposed {
    pub piece: PieceGuess,
    pub parts: Vec<PartGuess>,
}

/// Normalised text for comparisons: folded, letters and digits, single spaces.
pub fn norm(s: &str) -> String {
    fold(s).split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ")
}

fn push_candidate(list: &mut Vec<Candidate>, value: &Option<String>, same: fn(&str) -> String) {
    if let Some(v) = value.as_ref().filter(|v| !v.is_empty())
        && !list.iter().any(|c| same(&c.value) == same(v))
    {
        list.push(Candidate { value: v.clone(), source: Source::Pattern });
    }
}

/// Groups the paths into pieces: same archive number (ignoring leading
/// zeros and separators) or, without number, same title and composer.
/// Returns the pieces (by number, then title, natural order) and the paths
/// the pattern does not match. Paths are taken in byte order: the first
/// path gives the first (shown) candidate of each field.
pub fn propose(pattern: &Pattern, language: Language, paths: &[String]) -> (Vec<Proposed>, Vec<String>) {
    let mut pieces: Vec<Proposed> = Vec::new();
    let mut unmatched = Vec::new();
    // Path order, so preview and server pick the same first candidate.
    let mut paths: Vec<&String> = paths.iter().collect();
    paths.sort();
    for path in paths {
        let Some(f) = pattern.matches(path) else {
            unmatched.push(path.clone());
            continue;
        };
        let key = group_key(&f);
        let idx = match pieces.iter().position(|p| p.piece.key == key) {
            Some(i) => i,
            None => {
                pieces.push(Proposed { piece: PieceGuess { key, ..Default::default() }, parts: Vec::new() });
                pieces.len() - 1
            }
        };
        let piece = &mut pieces[idx];
        push_candidate(&mut piece.piece.number, &f.number, naming::archive_key);
        push_candidate(&mut piece.piece.title, &f.title, norm);
        push_candidate(&mut piece.piece.composer, &f.composer, norm);
        push_candidate(&mut piece.piece.arranger, &f.arranger, norm);
        push_candidate(&mut piece.piece.genre, &f.genre, norm);
        piece.parts.push(PartGuess { path: path.clone(), text: parse_part(&f.part, language) });
    }
    for p in &mut pieces {
        p.parts.sort_by(|a, b| naming::natural_cmp(&a.path, &b.path));
    }
    pieces.sort_by(|a, b| {
        let first = |c: &[Candidate]| c.first().map(|c| c.value.clone());
        match (first(&a.piece.number), first(&b.piece.number)) {
            (Some(x), Some(y)) => naming::natural_cmp(&x, &y),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        }
        .then_with(|| {
            naming::natural_cmp(&first(&a.piece.title).unwrap_or_default(), &first(&b.piece.title).unwrap_or_default())
        })
    });
    (pieces, unmatched)
}

fn group_key(f: &PathFields) -> String {
    match f.number.as_deref().map(naming::archive_key).filter(|k| !k.is_empty()) {
        Some(k) => format!("n:{k}"),
        None => format!(
            "t:{}|{}",
            norm(f.title.as_deref().unwrap_or_default()),
            norm(f.composer.as_deref().unwrap_or_default())
        ),
    }
}

// ---------------------------------------------------------------------------
// The librarian's changes
// ---------------------------------------------------------------------------

/// Where the parts of a proposed piece go.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Target {
    /// A new piece (the default when no existing piece matches).
    New,
    /// Add the parts to this existing piece (confirmed by the librarian).
    Existing(Id),
}

/// Fields the librarian changed on a piece (`Some("")` clears a value).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
pub struct PieceOverrides {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub composer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arranger: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub genre: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<Target>,
}

/// One instrument of a part as the librarian left it in the review
/// (a-import-instruments): an instrument of the library, or an instrument
/// word of the file name that is kept and resolved like the detection
/// (alias, library, dictionary — an unknown word stays red and goes through
/// "Instruments to confirm", it is never created by itself). JSON: a number
/// or a string (the word's alias key, e.g. `"klar"`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(untagged)]
pub enum InstrumentPick {
    Id(Id),
    Word(String),
}

/// At most this many instruments per part in the review.
pub const MAX_PART_INSTRUMENTS: usize = 20;

/// Fields the librarian changed on a part.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
pub struct PartOverrides {
    /// The instruments in order, replacing the detected ones; `Some([])` =
    /// none. (`instrument_ids` is accepted as the older name.)
    #[serde(default, alias = "instrument_ids", skip_serializing_if = "Option::is_none")]
    pub instruments: Option<Vec<InstrumentPick>>,
    /// Key code; `Some("")` = no key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// Voices in order; `Some([])` = no voice. Cleaned with
    /// `naming::clean_voices` ("1 & 2" typed as one entry is two voices).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voices: Option<Vec<String>>,
    /// Custom name; `Some("")` = derived name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// `Some(true)`: do not import this file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skip: Option<bool>,
}

impl PieceOverrides {
    /// Takes over every field `changes` sets (bulk edit, `PATCH /api/import/items`).
    pub fn merge(&mut self, changes: &PieceOverrides) {
        for (mine, theirs) in [
            (&mut self.number, &changes.number),
            (&mut self.title, &changes.title),
            (&mut self.composer, &changes.composer),
            (&mut self.arranger, &changes.arranger),
            (&mut self.genre, &changes.genre),
        ] {
            if let Some(v) = theirs {
                *mine = Some(v.trim().to_owned());
            }
        }
        if changes.target.is_some() {
            self.target = changes.target;
        }
    }
}

impl PartOverrides {
    /// Takes over every field `changes` sets.
    pub fn merge(&mut self, changes: &PartOverrides) {
        if let Some(picks) = &changes.instruments {
            let mut unique: Vec<InstrumentPick> = Vec::new();
            for p in picks {
                let p = match p {
                    InstrumentPick::Word(w) => InstrumentPick::Word(w.trim().to_owned()),
                    id => id.clone(),
                };
                if !unique.contains(&p) && p != InstrumentPick::Word(String::new()) {
                    unique.push(p);
                }
            }
            self.instruments = Some(unique);
        }
        if let Some(v) = &changes.voices {
            self.voices = Some(naming::clean_voices(v));
        }
        for (mine, theirs) in [(&mut self.key, &changes.key), (&mut self.label, &changes.label)] {
            if let Some(v) = theirs {
                *mine = Some(v.trim().to_owned());
            }
        }
        if changes.skip.is_some() {
            self.skip = changes.skip;
        }
    }
}

// ---------------------------------------------------------------------------
// Resolution
// ---------------------------------------------------------------------------

/// A stored instrument alias: file-name text (folded) ⇒ instrument.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Alias {
    pub alias: String,
    pub instrument_id: Id,
}

/// An existing piece, as far as the review needs it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ScoreRef {
    pub id: Id,
    pub title: String,
    pub composer: Option<String>,
    pub archive_number: Option<String>,
}

impl ScoreRef {
    pub fn label(&self) -> String {
        let mut s = String::new();
        if let Some(n) = &self.archive_number {
            s.push_str(&format!("No. {n} "));
        }
        s.push_str(&format!("\"{}\"", self.title));
        if let Some(c) = &self.composer {
            s.push_str(&format!(" ({c})"));
        }
        s
    }
}

/// What the review needs to know about the library.
#[derive(Clone, Copy, Debug)]
pub struct Library<'a> {
    pub instruments: &'a [Instrument],
    pub aliases: &'a [Alias],
    pub scores: &'a [ScoreRef],
    pub groups: &'a [Group],
}

/// The voices of a part with source, colour and note (a-part-voices).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
pub struct VoicesField {
    pub values: Vec<String>,
    pub source: Option<Source>,
    pub level: Level,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<Note>,
}

/// A shown value with source, colour and note.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
pub struct Field {
    pub value: Option<String>,
    pub source: Option<Source>,
    pub level: Level,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<Note>,
    /// Other values the sources offered.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub others: Vec<String>,
}

/// An instrument word of a part, resolved against the library.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ResolvedInstrument {
    /// As in the file name ("Klar"); empty if chosen by the librarian.
    pub text: String,
    /// Key for a new alias ("klar").
    pub alias: String,
    /// Dictionary name, the name proposed when creating it ("Clarinet").
    pub entry: Option<String>,
    pub instrument_id: Option<Id>,
    pub name: Option<String>,
    pub source: Option<Source>,
    pub level: Level,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<Note>,
}

/// Resolves one instrument word: stored alias, then an instrument with
/// exactly this name, then the dictionary (an instrument named like any
/// spelling of the dictionary entry); otherwise unknown (red).
pub fn resolve_instrument(text: &parts::InstrumentText, lib: &Library) -> ResolvedInstrument {
    let named = |id: Id| lib.instruments.iter().find(|i| i.id == id).map(|i| i.name.clone());
    let base = ResolvedInstrument {
        text: text.text.clone(),
        alias: text.alias.clone(),
        entry: text.entry.clone(),
        instrument_id: None,
        name: None,
        source: None,
        level: Level::Red,
        note: None,
    };
    if let Some(a) = lib.aliases.iter().find(|a| a.alias == text.alias)
        && let Some(name) = named(a.instrument_id)
    {
        return ResolvedInstrument {
            instrument_id: Some(a.instrument_id),
            name: Some(name),
            source: Some(Source::Alias),
            level: Level::Green,
            ..base
        };
    }
    if let Some(i) = lib.instruments.iter().find(|i| fold(&i.name) == text.alias) {
        let level = if text.level == Level::Yellow
            && text.entry.as_ref().is_some_and(|e| dictionary(e).is_some_and(|d| d.ambiguous))
        {
            Level::Yellow
        } else {
            Level::Green
        };
        let note = (level == Level::Yellow).then(|| Note::AmbiguousInstrument(text.text.clone()));
        return ResolvedInstrument {
            instrument_id: Some(i.id),
            name: Some(i.name.clone()),
            source: Some(Source::Library),
            level,
            note,
            ..base
        };
    }
    if let Some(entry) = text.entry.as_deref().and_then(dictionary) {
        let spellings: Vec<String> = std::iter::once(fold(entry.name))
            .chain(entry.words.iter().map(|(w, _)| w.to_string()))
            .chain(entry.abbreviations.iter().map(|a| a.to_string()))
            .collect();
        let found = lib
            .instruments
            .iter()
            .find(|i| fold(&i.name) == fold(entry.name))
            .or_else(|| lib.instruments.iter().find(|i| spellings.contains(&fold(&i.name))));
        if let Some(i) = found {
            let note = match text.level {
                Level::Green => None,
                _ if entry.ambiguous => Some(Note::AmbiguousInstrument(text.text.clone())),
                _ if entry.abbreviations.contains(&text.alias.as_str()) => Some(Note::Abbreviation(text.text.clone())),
                _ => Some(Note::Guessed(text.text.clone())),
            };
            return ResolvedInstrument {
                instrument_id: Some(i.id),
                name: Some(i.name.clone()),
                source: Some(Source::Dictionary),
                level: text.level.min(Level::Yellow),
                note,
                ..base
            };
        }
    }
    ResolvedInstrument { note: Some(Note::UnknownInstrument(text.text.clone())), ..base }
}

fn dictionary(name: &str) -> Option<&'static parts::Entry> {
    DICTIONARY.iter().find(|e| e.name == name)
}

/// State of an uploaded file of the batch.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FileState {
    /// Listed, content not received yet.
    Pending,
    Uploaded,
    /// The same file is already in the library or elsewhere in the batch: skipped.
    Duplicate,
    Failed,
}

impl FileState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Uploaded => "uploaded",
            Self::Duplicate => "duplicate",
            Self::Failed => "failed",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "uploaded" => Self::Uploaded,
            "duplicate" => Self::Duplicate,
            "failed" => Self::Failed,
            _ => Self::Pending,
        }
    }
}

/// A file of a batch.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct StagedFile {
    pub id: Id,
    pub path: String,
    pub size_bytes: i64,
    pub sha256: String,
    pub state: FileState,
    pub page_count: Option<i64>,
    pub has_thumbnail: bool,
    /// Staves per system of the file (median), once the system detection ran.
    pub staves: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Typical number of staves per system: the most frequent count among the
/// detected systems of pages not shown whole (ties: the larger count). The
/// server computes the same in SQL (`routes/import.rs`, `STAVES`).
pub fn staves_per_system(pages: &[PageLayout]) -> Option<u32> {
    let mut counts: Vec<(u32, usize)> = Vec::new();
    for n in pages
        .iter()
        .filter(|p| !p.full_page)
        .flat_map(|p| p.regions.iter().filter(|r| r.kind == RegionKind::System).map(|r| r.staves))
        .filter(|n| *n > 0)
    {
        match counts.iter_mut().find(|(c, _)| *c == n) {
            Some((_, k)) => *k += 1,
            None => counts.push((n, 1)),
        }
    }
    counts.into_iter().max_by_key(|(n, k)| (*k, *n)).map(|(n, _)| n)
}

/// State of a proposed piece or part.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ItemState {
    Pending,
    Committed,
}

/// One part row of the review.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ReviewPart {
    pub id: Id,
    pub file: StagedFile,
    /// The `{part}` text of the file name.
    pub text: String,
    pub instruments: Vec<ResolvedInstrument>,
    pub key: Field,
    pub voices: VoicesField,
    /// Custom name chosen in the review.
    pub label: Option<String>,
    /// The name the part will have.
    pub display_name: String,
    /// Groups that will be able to read the part.
    pub readers: Vec<String>,
    pub notes: Vec<Note>,
    pub level: Level,
    pub skip: bool,
    pub state: ItemState,
    /// The created part, after the commit.
    pub part_id: Option<Id>,
}

/// One piece row of the review.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ReviewPiece {
    pub id: Id,
    pub state: ItemState,
    pub level: Level,
    pub number: Field,
    pub title: Field,
    pub composer: Field,
    pub arranger: Field,
    pub genre: Field,
    /// An existing piece that looks like this one (same archive number, or
    /// same title and composer).
    pub suggestion: Option<ScoreRef>,
    /// The librarian's choice; `None` = not decided (new piece if there is no suggestion).
    pub target: Option<Target>,
    /// The piece the parts went to (after the commit) or will go to (confirmed existing piece).
    pub score: Option<ScoreRef>,
    pub notes: Vec<Note>,
    pub parts: Vec<ReviewPart>,
}

impl ReviewPart {
    /// The shown instruments as an override (a-import-instruments): chosen
    /// ones by id, detected ones as their word, so that adding or removing
    /// one keeps the others as they are (an unknown word stays to confirm).
    pub fn instrument_picks(&self) -> Vec<InstrumentPick> {
        self.instruments.iter().filter_map(ResolvedInstrument::pick).collect()
    }
}

impl ResolvedInstrument {
    /// This instrument as an override entry: chosen ones by id, detected
    /// ones as their word (see [`ReviewPart::instrument_picks`]).
    pub fn pick(&self) -> Option<InstrumentPick> {
        match (self.source, self.instrument_id) {
            (Some(Source::Librarian), Some(id)) => Some(InstrumentPick::Id(id)),
            _ if !self.alias.is_empty() => Some(InstrumentPick::Word(self.alias.clone())),
            (_, Some(id)) => Some(InstrumentPick::Id(id)),
            _ => None,
        }
    }
}

impl ReviewPiece {
    /// Ready for "Commit all green"?
    pub fn is_green(&self) -> bool {
        self.state == ItemState::Pending && self.level == Level::Green
    }

    /// May be committed by itself (yellow allowed, red not)?
    pub fn can_commit(&self) -> bool {
        self.state == ItemState::Pending && self.level < Level::Red
    }

    /// The first thing that keeps the piece red (for messages).
    pub fn blocking_note(&self) -> Option<Note> {
        let fields = [&self.number, &self.title, &self.composer, &self.arranger, &self.genre];
        let red_field = fields.iter().find(|f| f.level == Level::Red).and_then(|f| f.note.clone());
        let red_part = self.parts.iter().filter(|p| !p.skip && p.level == Level::Red).find_map(|p| {
            p.instruments
                .iter()
                .find(|i| i.level == Level::Red)
                .and_then(|i| i.note.clone())
                .or_else(|| (p.key.level == Level::Red).then(|| p.key.note.clone()).flatten())
                .or_else(|| (p.voices.level == Level::Red).then(|| p.voices.note.clone()).flatten())
                .or_else(|| p.notes.first().cloned())
        });
        red_field.or(red_part).or_else(|| self.notes.first().cloned())
    }

    /// Where the parts go: an existing piece or a new one.
    pub fn effective_target(&self) -> Target {
        match self.target {
            Some(t) => t,
            None => Target::New,
        }
    }
}

/// Input of [`review_piece`] for one part.
#[derive(Clone, Debug)]
pub struct PartInput<'a> {
    pub id: Id,
    pub guess: &'a PartGuess,
    pub overrides: &'a PartOverrides,
    pub file: StagedFile,
    pub state: ItemState,
    pub part_id: Option<Id>,
}

fn field(candidates: &[Candidate], over: Option<&String>) -> Field {
    if let Some(v) = over {
        let v = v.trim();
        return Field {
            value: (!v.is_empty()).then(|| v.to_owned()),
            source: Some(Source::Librarian),
            level: Level::Green,
            note: None,
            others: candidates.iter().map(|c| c.value.clone()).filter(|c| c != v).collect(),
        };
    }
    match candidates {
        [] => Field::default(),
        [one] => Field { value: Some(one.value.clone()), source: Some(one.source), ..Field::default() },
        [first, rest @ ..] => Field {
            value: Some(first.value.clone()),
            source: Some(first.source),
            level: Level::Red,
            note: Some(Note::Contradictory(
                candidates.iter().map(|c| c.value.as_str()).collect::<Vec<_>>().join(" / "),
            )),
            others: rest.iter().map(|c| c.value.clone()).collect(),
        },
    }
}

/// The key of a part: the librarian's choice, else what the name says.
fn key_field(reading: &parts::Reading, over: Option<&String>) -> Field {
    if let Some(v) = over {
        let v = v.trim();
        if v.is_empty() {
            return Field { source: Some(Source::Librarian), ..Field::default() };
        }
        if naming::normalize_key(v).is_none() {
            return Field {
                value: Some(v.to_owned()),
                source: Some(Source::Librarian),
                level: Level::Red,
                note: Some(Note::InvalidKey(v.to_owned())),
                others: vec![],
            };
        }
        let value = naming::normalize_key(v).map(str::to_owned);
        return Field { value, source: Some(Source::Librarian), ..Field::default() };
    }
    Field {
        value: reading.value.clone(),
        source: reading.value.as_ref().map(|_| Source::Pattern),
        level: reading.level,
        note: reading.note.clone(),
        others: vec![],
    }
}

/// The review row of a piece.
pub fn review_piece(
    id: Id,
    state: ItemState,
    guess: &PieceGuess,
    over: &PieceOverrides,
    committed_score: Option<Id>,
    parts: Vec<PartInput>,
    lib: &Library,
) -> ReviewPiece {
    let number = field(&guess.number, over.number.as_ref());
    let mut title = field(&guess.title, over.title.as_ref());
    let composer = field(&guess.composer, over.composer.as_ref());
    let arranger = field(&guess.arranger, over.arranger.as_ref());
    let genre = field(&guess.genre, over.genre.as_ref());
    let score_by_id = |id: Id| lib.scores.iter().find(|s| s.id == id).cloned();

    // An existing piece with the same number, or the same title and composer.
    let suggestion = if state == ItemState::Committed {
        None
    } else {
        let key = number.value.as_deref().map(naming::archive_key).filter(|k| !k.is_empty());
        let t = title.value.as_deref().map(norm).unwrap_or_default();
        let c = composer.value.as_deref().map(norm).unwrap_or_default();
        lib.scores
            .iter()
            .find(|s| key.is_some() && s.archive_number.as_deref().map(naming::archive_key) == key)
            .or_else(|| {
                lib.scores.iter().find(|s| {
                    !t.is_empty() && norm(&s.title) == t && norm(s.composer.as_deref().unwrap_or_default()) == c
                })
            })
            .cloned()
    };
    let mut notes = Vec::new();
    let mut level = Level::Green;
    let target = over.target;
    let score = match (state, committed_score, target) {
        (ItemState::Committed, Some(id), _) => score_by_id(id),
        (_, _, Some(Target::Existing(id))) => {
            let s = score_by_id(id);
            if s.is_none() {
                notes.push(Note::TargetGone);
                level = Level::Red;
            }
            s
        }
        _ => None,
    };
    if state == ItemState::Pending {
        if let (Some(s), None) = (&suggestion, target) {
            notes.push(Note::ConfirmTarget(s.label()));
            level = level.max(Level::Yellow);
        }
        let new_piece = !matches!(target, Some(Target::Existing(_)));
        if new_piece && title.value.is_none() {
            title.level = Level::Red;
            title.note = Some(Note::TitleMissing);
        }
        // A piece with the same title but another composer: maybe the same work.
        if new_piece
            && title.level == Level::Green
            && let Some(t) = title.value.as_deref().map(norm)
            && let Some(s) =
                lib.scores.iter().find(|s| norm(&s.title) == t && suggestion.as_ref().is_none_or(|x| x.id != s.id))
        {
            title.level = Level::Yellow;
            title.note = Some(Note::SimilarTitle(s.label()));
        }
    }
    for f in [&number, &title, &composer, &arranger, &genre] {
        level = level.max(f.level);
    }

    let extra_readers: Vec<&Group> = match score.as_ref() {
        Some(s) => lib.groups.iter().filter(|g| g.score_ids.contains(&s.id)).collect(),
        None => Vec::new(),
    };
    let parts: Vec<ReviewPart> = parts.into_iter().map(|p| review_part(p, &extra_readers, lib)).collect();
    let active: Vec<&ReviewPart> = parts.iter().filter(|p| !p.skip && p.state == ItemState::Pending).collect();
    if state == ItemState::Pending {
        if active.is_empty() {
            notes.push(Note::NoParts);
            level = Level::Red;
        }
        for p in &active {
            level = level.max(p.level);
        }
    }
    ReviewPiece { id, state, level, number, title, composer, arranger, genre, suggestion, target, score, notes, parts }
}

fn review_part(p: PartInput, extra_readers: &[&Group], lib: &Library) -> ReviewPart {
    let text = &p.guess.text;
    let mut notes = Vec::new();
    let instruments: Vec<ResolvedInstrument> = match &p.overrides.instruments {
        Some(picks) => {
            let mut list: Vec<ResolvedInstrument> = Vec::new();
            for pick in picks {
                let r = match pick {
                    // Chosen by the librarian: sure.
                    InstrumentPick::Id(id) => match lib.instruments.iter().find(|i| i.id == *id) {
                        Some(i) => ResolvedInstrument {
                            text: String::new(),
                            alias: String::new(),
                            entry: None,
                            instrument_id: Some(i.id),
                            name: Some(i.name.clone()),
                            source: Some(Source::Librarian),
                            level: Level::Green,
                            note: None,
                        },
                        None => continue,
                    },
                    // Kept from the file name: as sure as the detection.
                    InstrumentPick::Word(w) => match text.instruments.iter().find(|t| t.alias == *w) {
                        Some(t) => resolve_instrument(t, lib),
                        None => continue,
                    },
                };
                let twice = r.instrument_id.is_some() && list.iter().any(|x| x.instrument_id == r.instrument_id);
                if !twice {
                    list.push(r);
                }
            }
            list
        }
        None => text.instruments.iter().map(|t| resolve_instrument(t, lib)).collect(),
    };
    let key = key_field(&text.key, p.overrides.key.as_ref());
    let voices = match &p.overrides.voices {
        Some(v) => {
            VoicesField { values: naming::clean_voices(v), source: Some(Source::Librarian), ..Default::default() }
        }
        None => VoicesField {
            values: text.voices.values.clone(),
            source: (!text.voices.values.is_empty()).then_some(Source::Pattern),
            level: text.voices.level,
            note: text.voices.note.clone(),
        },
    };
    let mut level = key.level.max(voices.level);
    for i in &instruments {
        level = level.max(i.level);
    }
    if instruments.is_empty() {
        notes.push(Note::NoInstrument);
        level = Level::Red;
    }
    if !text.rest.is_empty() && p.overrides.instruments.is_none() {
        notes.push(Note::Unrecognised(text.rest.join(" ")));
        level = level.max(Level::Yellow);
    }
    match p.file.state {
        FileState::Pending => {
            notes.push(Note::NotUploaded);
            level = Level::Red;
        }
        FileState::Failed => {
            notes.push(Note::UploadFailed(p.file.message.clone().unwrap_or_default()));
            level = Level::Red;
        }
        _ => {}
    }
    // Cross-check with the detected systems.
    let names: Vec<String> = instruments.iter().filter_map(|i| i.name.clone()).collect();
    let entries: Vec<&str> = instruments.iter().filter_map(|i| i.entry.as_deref()).collect();
    let is = |n: &str| entries.contains(&n) || names.iter().any(|x| fold(x) == fold(n));
    if let Some(staves) = p.file.staves {
        if is("Partitur") && staves <= 1 {
            notes.push(Note::ScoreWithOneStaff);
            level = level.max(Level::Yellow);
        } else if instruments.len() == 1 && !is("Partitur") && !is("Direktion") && staves >= 4 {
            notes.push(Note::ManyStaves(staves));
            level = level.max(Level::Yellow);
        }
    }
    let ids: Vec<Id> = instruments.iter().filter_map(|i| i.instrument_id).collect();
    let mut readers: Vec<String> = lib
        .groups
        .iter()
        .filter(|g| g.instrument_ids.iter().any(|i| ids.contains(i)) || extra_readers.iter().any(|x| x.id == g.id))
        .map(|g| g.name.clone())
        .collect();
    readers.sort_by(|a, b| naming::natural_cmp(a, b));
    if readers.is_empty() && !ids.is_empty() {
        notes.push(Note::NoReaders);
    }
    let label = p.overrides.label.as_ref().map(|l| l.trim().to_owned()).filter(|l| !l.is_empty());
    let name_refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let display_name = naming::part_name(label.as_deref(), &name_refs, key.value.as_deref(), &voices.values);
    ReviewPart {
        id: p.id,
        file: p.file,
        text: text.text.clone(),
        instruments,
        key,
        voices,
        label,
        display_name,
        readers,
        notes,
        level,
        skip: p.overrides.skip == Some(true),
        state: p.state,
        part_id: p.part_id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::pattern::PRESETS;

    fn instruments(names: &[&str]) -> Vec<Instrument> {
        names
            .iter()
            .enumerate()
            .map(|(i, n)| Instrument { id: i as Id + 1, name: n.to_string(), family: None })
            .collect()
    }

    fn file(id: Id) -> StagedFile {
        StagedFile {
            id,
            path: String::new(),
            size_bytes: 1,
            sha256: String::new(),
            state: FileState::Uploaded,
            page_count: Some(1),
            has_thumbnail: true,
            staves: None,
            message: None,
        }
    }

    fn review(proposed: &Proposed, over: &PieceOverrides, lib: &Library) -> ReviewPiece {
        let none = PartOverrides::default();
        let parts = proposed
            .parts
            .iter()
            .enumerate()
            .map(|(i, g)| PartInput {
                id: i as Id,
                guess: g,
                overrides: &none,
                file: file(i as Id),
                state: ItemState::Pending,
                part_id: None,
            })
            .collect();
        review_piece(1, ItemState::Pending, &proposed.piece, over, None, parts, lib)
    }

    fn paths(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn files_are_grouped_into_pieces() {
        let p = Pattern::parse(PRESETS[0].pattern).unwrap();
        let (pieces, unmatched) = propose(
            &p,
            Language::De,
            &paths(&[
                "Märsche/0304 Alte Kameraden - Teike/Partitur.pdf",
                "Märsche/0304 Alte Kameraden - Teike/Direktion.pdf",
                "Märsche/0304 Alte Kameraden - Teike/Klarinette 1 in B.pdf",
                "Polkas/28 Böhmischer Traum/Trompete 1+2.pdf",
                "Ohne Nummer/Tuba.pdf",
                "lose.pdf",
            ]),
        );
        assert_eq!(unmatched, ["lose.pdf"]);
        assert_eq!(pieces.len(), 3);
        // Natural order by number: 28 before 0304, without number last.
        assert_eq!(pieces[0].piece.number[0].value, "28");
        assert_eq!(pieces[1].piece.title[0].value, "Alte Kameraden");
        assert_eq!(pieces[1].piece.composer[0].value, "Teike");
        assert_eq!(pieces[1].piece.genre[0].value, "Märsche");
        assert_eq!(pieces[1].parts.len(), 3);
        assert_eq!(pieces[2].piece.key, "t:ohne nummer|");
    }

    #[test]
    fn flat_files_group_by_number_and_need_a_title() {
        let p = Pattern::parse(PRESETS[3].pattern).unwrap();
        let (pieces, _) = propose(
            &p,
            Language::De,
            &paths(&["304Stimme06_Klarinette2.pdf", "0304Stimme07_KlarinetteEb.pdf", "28Stimme04_KlarinetteEb.pdf"]),
        );
        assert_eq!(pieces.len(), 2, "304 and 0304 are the same piece");
        let instr = instruments(&["Klarinette"]);
        let lib = Library { instruments: &instr, aliases: &[], scores: &[], groups: &[] };
        let r = review(&pieces[1], &PieceOverrides::default(), &lib);
        assert_eq!(r.number.value.as_deref(), Some("0304"), "first path in order");
        assert_eq!((r.title.level, r.title.note.clone()), (Level::Red, Some(Note::TitleMissing)));
        assert_eq!(r.level, Level::Red);
        // Typed once for the piece: green.
        let typed = PieceOverrides { title: Some("Alte Kameraden".into()), ..Default::default() };
        let r = review(&pieces[1], &typed, &lib);
        assert_eq!(
            (r.title.value.as_deref(), r.title.source, r.level),
            (Some("Alte Kameraden"), Some(Source::Librarian), Level::Green)
        );
        assert_eq!(
            r.parts.iter().map(|p| p.display_name.as_str()).collect::<Vec<_>>(),
            ["Klarinette 2", "Klarinette in E♭"]
        );
    }

    #[test]
    fn instruments_resolve_through_aliases_names_and_the_dictionary() {
        let instr = instruments(&["Klarinette", "Trumpet", "Partitur", "Direktion"]);
        let none: Vec<Alias> = vec![];
        let lib = Library { instruments: &instr, aliases: &none, scores: &[], groups: &[] };
        let r = |t: &str| resolve_instrument(&parse_part(t, Language::De).instruments[0], &lib);
        let k = r("Klarinette 1");
        assert_eq!((k.instrument_id, k.source, k.level), (Some(1), Some(Source::Library), Level::Green));
        let t = r("Trompete 2");
        assert_eq!((t.instrument_id, t.source, t.level), (Some(2), Some(Source::Dictionary), Level::Green));
        let a = r("Klar 2");
        assert_eq!(
            (a.instrument_id, a.level, a.note.clone()),
            (Some(1), Level::Yellow, Some(Note::Abbreviation("Klar".into())))
        );
        let u = r("Alphorn");
        assert_eq!((u.instrument_id, u.level, u.alias.as_str()), (None, Level::Red, "alphorn"));
        // Partitur and Direktion map to their own instruments.
        assert_eq!(r("Partitur").instrument_id, Some(3));
        assert_eq!(r("Full score").instrument_id, Some(3));
        assert_eq!(r("Direktion").instrument_id, Some(4));
        assert_eq!(r("Conductor").instrument_id, Some(4));
        // A stored alias makes an abbreviation (or an unknown word) green.
        let aliases =
            vec![Alias { alias: "klar".into(), instrument_id: 1 }, Alias { alias: "alphorn".into(), instrument_id: 2 }];
        let lib = Library { aliases: &aliases, ..lib };
        let r = |t: &str| resolve_instrument(&parse_part(t, Language::De).instruments[0], &lib);
        assert_eq!((r("Klar 2").level, r("Klar 2").source), (Level::Green, Some(Source::Alias)));
        assert_eq!(r("Alphorn").instrument_id, Some(2));
    }

    #[test]
    fn partitur_and_direktion_need_their_own_instruments() {
        let instr = instruments(&["Trumpet"]);
        let lib = Library { instruments: &instr, aliases: &[], scores: &[], groups: &[] };
        for t in ["Partitur", "Direktion"] {
            let r = resolve_instrument(&parse_part(t, Language::De).instruments[0], &lib);
            assert_eq!(
                (r.instrument_id, r.level, r.entry.as_deref()),
                (None, Level::Red, Some(t)),
                "never mapped to another instrument"
            );
        }
    }

    #[test]
    fn existing_pieces_are_suggested_not_chosen() {
        let p = Pattern::parse(PRESETS[0].pattern).unwrap();
        let (pieces, _) = propose(
            &p,
            Language::De,
            &paths(&[
                "0304 Alte Kameraden/Tuba.pdf",
                "Böhmischer Traum - Fr. Pischelsrieder/Tuba.pdf",
                "Florentiner/Tuba.pdf",
            ]),
        );
        let instr = instruments(&["Tuba"]);
        let scores = vec![
            ScoreRef {
                id: 7,
                title: "Alte Kameraden".into(),
                composer: Some("Teike".into()),
                archive_number: Some("304".into()),
            },
            ScoreRef {
                id: 8,
                title: "Böhmischer Traum".into(),
                composer: Some("Fr. Pischelsrieder".into()),
                archive_number: None,
            },
            ScoreRef { id: 9, title: "Florentiner".into(), composer: Some("Fučík".into()), archive_number: None },
        ];
        let groups = vec![
            Group { id: 1, name: "Tubas".into(), member_ids: vec![], instrument_ids: vec![1], score_ids: vec![] },
            Group { id: 2, name: "Conductors".into(), member_ids: vec![], instrument_ids: vec![], score_ids: vec![7] },
        ];
        let lib = Library { instruments: &instr, aliases: &[], scores: &scores, groups: &groups };
        let by_number = review(&pieces[0], &PieceOverrides::default(), &lib);
        assert_eq!(by_number.suggestion.as_ref().map(|s| s.id), Some(7));
        assert_eq!(by_number.level, Level::Yellow, "needs a confirmation");
        let by_title = review(&pieces[1], &PieceOverrides::default(), &lib);
        assert_eq!(by_title.suggestion.as_ref().map(|s| s.id), Some(8));
        // Same title, other composer: only a hint on the title.
        let similar = review(&pieces[2], &PieceOverrides::default(), &lib);
        assert_eq!((similar.suggestion.clone(), similar.title.level), (None, Level::Yellow));
        // Confirmed: parts go to the existing piece; its score grants count for the readers.
        let confirmed =
            review(&pieces[0], &PieceOverrides { target: Some(Target::Existing(7)), ..Default::default() }, &lib);
        assert_eq!((confirmed.level, confirmed.score.as_ref().map(|s| s.id)), (Level::Green, Some(7)));
        assert_eq!(confirmed.parts[0].readers, ["Conductors", "Tubas"]);
        let new = review(&pieces[0], &PieceOverrides { target: Some(Target::New), ..Default::default() }, &lib);
        assert_eq!(new.suggestion.as_ref().map(|s| s.id), Some(7), "still shown");
        assert_eq!((new.level, new.effective_target()), (Level::Green, Target::New));
        assert_eq!(new.parts[0].readers, ["Tubas"]);
    }

    #[test]
    fn changes_are_merged() {
        let mut piece = PieceOverrides { title: Some("A".into()), ..Default::default() };
        piece.merge(&PieceOverrides { genre: Some(" March ".into()), target: Some(Target::New), ..Default::default() });
        assert_eq!(
            (piece.title.as_deref(), piece.genre.as_deref(), piece.target),
            (Some("A"), Some("March"), Some(Target::New))
        );
        let mut part = PartOverrides { key: Some("Bb".into()), skip: Some(true), ..Default::default() };
        let picks = vec![InstrumentPick::Id(3), InstrumentPick::Word(" klar ".into()), InstrumentPick::Id(3)];
        part.merge(&PartOverrides { instruments: Some(picks), skip: Some(false), ..Default::default() });
        assert_eq!(
            (part.key.as_deref(), part.instruments, part.skip),
            (Some("Bb"), Some(vec![InstrumentPick::Id(3), InstrumentPick::Word("klar".into())]), Some(false)),
            "duplicates removed, words trimmed"
        );
        let json =
            serde_json::to_string(&PieceOverrides { target: Some(Target::Existing(5)), ..Default::default() }).unwrap();
        assert_eq!(json, r#"{"target":{"existing":5}}"#);
    }

    #[test]
    fn typical_staves() {
        use crate::layout::Region;
        let region = |kind, staves| Region {
            kind,
            y0: 0.0,
            y1: 0.1,
            ink_y0: 0.0,
            ink_y1: 0.1,
            x0: 0.0,
            x1: 1.0,
            staves,
            staff_space: 0.01,
        };
        let page = |staves: &[u32], full_page| PageLayout {
            width_mm: 210.0,
            height_mm: 297.0,
            confidence: 1.0,
            full_page,
            regions: std::iter::once(region(RegionKind::Header, 0))
                .chain(staves.iter().map(|n| region(RegionKind::System, *n)))
                .collect(),
            corrected: false,
        };
        assert_eq!(staves_per_system(&[page(&[1, 1, 2], false), page(&[2, 2, 2], false), page(&[9], true)]), Some(2));
        assert_eq!(staves_per_system(&[page(&[1, 2], false)]), Some(2), "tie: the larger");
        assert_eq!(staves_per_system(&[page(&[], false)]), None);
    }

    #[test]
    fn contradictions_overrides_and_layout_warnings() {
        let p = Pattern::parse("{number} {title}/{part}").unwrap();
        let (pieces, _) = propose(&p, Language::En, &paths(&["12 March/Trumpet.pdf", "012 Marsch/Score.pdf"]));
        assert_eq!(pieces.len(), 1);
        let instr = instruments(&["Trumpet", "Partitur"]);
        let lib = Library { instruments: &instr, aliases: &[], scores: &[], groups: &[] };
        let r = review(&pieces[0], &PieceOverrides::default(), &lib);
        assert_eq!(
            (r.title.level, r.title.value.as_deref(), r.title.others.clone()),
            (Level::Red, Some("Marsch"), vec!["March".to_string()])
        );
        assert!(matches!(r.title.note, Some(Note::Contradictory(_))));
        let fixed = review(&pieces[0], &PieceOverrides { title: Some("March".into()), ..Default::default() }, &lib);
        assert_eq!(fixed.level, Level::Green);

        // Staves per system against the instrument.
        let none = PartOverrides::default();
        let mut f = file(1);
        f.staves = Some(1);
        let entry = |g: &&PartGuess| g.text.instruments[0].entry.clone();
        let score = pieces[0].parts.iter().find(|g| entry(g).as_deref() == Some("Partitur")).unwrap();
        let trumpet = pieces[0].parts.iter().find(|g| entry(g).as_deref() == Some("Trumpet")).unwrap();
        let part = |guess, file: StagedFile, over: &PartOverrides| {
            review_part(
                PartInput { id: 1, guess, overrides: over, file, state: ItemState::Pending, part_id: None },
                &[],
                &lib,
            )
        };
        assert!(part(score, f.clone(), &none).notes.contains(&Note::ScoreWithOneStaff));
        f.staves = Some(5);
        let many = part(trumpet, f.clone(), &none);
        assert!(many.notes.contains(&Note::ManyStaves(5)) && many.notes.contains(&Note::NoReaders));
        assert_eq!(many.level, Level::Yellow, "no readers is only a hint");
        // Key and voices typed by the librarian; an unknown key is red.
        let over =
            PartOverrides { key: Some("in B-flat".into()), voices: Some(vec!["2".into()]), ..Default::default() };
        let r = part(trumpet, file(1), &over);
        assert_eq!((r.display_name.as_str(), r.level), ("Trumpet in B♭ 2", Level::Green));
        let mut two = PartOverrides::default();
        two.merge(&PartOverrides { voices: Some(vec![" 1 & 2".into(), "1".into()]), ..Default::default() });
        assert_eq!(two.voices, Some(vec!["1".to_string(), "2".to_string()]), "cleaned on merge");
        let r = part(trumpet, file(1), &two);
        assert_eq!(r.voices.values, ["1", "2"], "cleaned");
        assert_eq!((r.display_name.as_str(), r.voices.source), ("Trumpet 1 & 2", Some(Source::Librarian)));
        let bad = PartOverrides { key: Some("B".into()), ..Default::default() };
        assert_eq!(part(trumpet, file(1), &bad).level, Level::Red);
        let mut pending = file(1);
        pending.state = FileState::Pending;
        assert_eq!(part(trumpet, pending, &none).level, Level::Red);
    }

    #[test]
    fn several_instruments_and_voices_chosen_in_the_review() {
        let p = Pattern::parse("{title}/{part}").unwrap();
        let (pieces, _) = propose(&p, Language::De, &paths(&["Marsch/Klar 2.pdf"]));
        let guess = &pieces[0].parts[0];
        assert_eq!(guess.text.instruments[0].alias, "klar");
        let instr = instruments(&["Trumpet", "Flute"]);
        let (trumpet, flute) = (instr[0].id, instr[1].id);
        let lib = Library { instruments: &instr, aliases: &[], scores: &[], groups: &[] };
        let part = |over: &PartOverrides, lib: &Library| {
            review_part(
                PartInput { id: 1, guess, overrides: over, file: file(1), state: ItemState::Pending, part_id: None },
                &[],
                lib,
            )
        };
        // Two instruments of the library and two voices: green, both in the name.
        let mut over = PartOverrides::default();
        over.merge(&PartOverrides {
            instruments: Some(vec![
                InstrumentPick::Id(trumpet),
                InstrumentPick::Id(flute),
                InstrumentPick::Id(trumpet),
            ]),
            voices: Some(vec!["1".into(), "2".into()]),
            ..Default::default()
        });
        let r = part(&over, &lib);
        let names: Vec<_> = r.instruments.iter().map(|i| (i.name.clone().unwrap(), i.source, i.level)).collect();
        assert_eq!(
            names,
            [
                ("Trumpet".to_string(), Some(Source::Librarian), Level::Green),
                ("Flute".to_string(), Some(Source::Librarian), Level::Green)
            ]
        );
        assert_eq!((r.level, r.display_name.as_str()), (Level::Green, "Flute / Trumpet 1 & 2"));
        assert_eq!(r.instrument_picks(), [InstrumentPick::Id(trumpet), InstrumentPick::Id(flute)]);

        // An instrument added to an unknown word keeps the word: still red,
        // still to confirm; a word the file name does not contain is ignored.
        let keep = PartOverrides {
            instruments: Some(vec![
                InstrumentPick::Word("klar".into()),
                InstrumentPick::Id(flute),
                InstrumentPick::Word("oboe".into()),
                InstrumentPick::Id(999),
            ]),
            ..Default::default()
        };
        let r = part(&keep, &lib);
        assert_eq!(r.instruments.len(), 2);
        assert_eq!((r.instruments[0].level, r.instruments[0].text.as_str()), (Level::Red, "Klar"));
        assert_eq!(r.level, Level::Red);
        assert_eq!(r.instrument_picks(), [InstrumentPick::Word("klar".into()), InstrumentPick::Id(flute)]);
        // Once the clarinet exists the kept word resolves like the detection.
        let more = instruments(&["Trumpet", "Flute", "Clarinet"]);
        let lib2 = Library { instruments: &more, aliases: &[], scores: &[], groups: &[] };
        let r = part(&keep, &lib2);
        assert_eq!(r.instruments[0].name.as_deref(), Some("Clarinet"));
        assert_ne!(r.instruments[0].level, Level::Red);
        // Removing every instrument: red (a part needs one).
        let none = PartOverrides { instruments: Some(vec![]), ..Default::default() };
        assert!(part(&none, &lib).notes.contains(&Note::NoInstrument));

        // JSON: numbers and words; the older name `instrument_ids` still reads.
        let o: PartOverrides = serde_json::from_str(r#"{"instruments":[2,"klar"]}"#).unwrap();
        assert_eq!(o.instruments, Some(vec![InstrumentPick::Id(2), InstrumentPick::Word("klar".into())]));
        let o: PartOverrides = serde_json::from_str(r#"{"instrument_ids":[2]}"#).unwrap();
        assert_eq!(o.instruments, Some(vec![InstrumentPick::Id(2)]));
        assert_eq!(serde_json::to_string(&o).unwrap(), r#"{"instruments":[2]}"#);
    }
}
