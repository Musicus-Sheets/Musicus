//! JSON request and response bodies of the REST API.
//!
//! Timestamps are Unix seconds (UTC). Dates without a time (concert dates)
//! are ISO strings `YYYY-MM-DD`.

use crate::error::ErrorCode;
use serde::{Deserialize, Deserializer, Serialize};

pub type Id = i64;

/// Header that every state-changing request authenticated by the session
/// cookie must carry. Browsers do not send custom headers cross-site without
/// a CORS preflight, so this blocks cross-site request forgery.
pub const CSRF_HEADER: &str = "x-musicus-csrf";

/// Name of the session cookie set by the web login.
pub const SESSION_COOKIE: &str = "musicus_session";

/// Global role of a user. Access to sheets is granted to groups, not roles;
/// roles only decide who may manage what.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// Reads the parts granted to their groups and annotates them.
    Member,
    /// Uploads files, edits metadata, programmes and grants.
    Librarian,
    /// Manages users and groups, plus everything a librarian may do.
    Admin,
}

impl Role {
    pub fn can_manage_library(self) -> bool {
        matches!(self, Role::Librarian | Role::Admin)
    }

    pub fn is_admin(self) -> bool {
        self == Role::Admin
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Role::Member => "member",
            Role::Librarian => "librarian",
            Role::Admin => "admin",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "member" => Some(Role::Member),
            "librarian" => Some(Role::Librarian),
            "admin" => Some(Role::Admin),
            _ => None,
        }
    }
}

/// Error body of every non-2xx response (except the page conflict):
/// `{"error": "bad_request", "code": "…", "args": {…}, "message": "…"}`
/// (a-error-codes, R-I18N-004).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ApiError {
    /// Generic class of the HTTP status, e.g. `not_found`, `conflict`.
    pub error: String,
    /// The specific reason with its arguments (`code` + `args`); clients show
    /// their own text for it.
    #[serde(flatten)]
    pub code: ErrorCode,
    /// English, for API users and logs only: always `code.to_string()`.
    pub message: String,
}

impl ApiError {
    pub fn new(error: &str, code: ErrorCode) -> Self {
        ApiError { error: error.to_owned(), message: code.to_string(), code }
    }
}

// ---------------------------------------------------------------------------
// Authentication
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
    /// `true` for native apps: the token is returned in the body and no
    /// cookie is set. Web clients leave this `false`.
    #[serde(default)]
    pub bearer: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LoginResponse {
    pub user: CurrentUser,
    /// Only present when the login asked for a bearer token.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CurrentUser {
    pub id: Id,
    pub username: String,
    pub display_name: String,
    pub role: Role,
    /// Reader mode and staff size, stored on the server (nothing on the device).
    #[serde(default)]
    pub reader: crate::layout::ReaderSettings,
}

/// What the invite page shows before the user chooses a password.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct InviteInfo {
    pub username: String,
    pub display_name: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AcceptInvite {
    pub password: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChangePassword {
    pub current_password: String,
    pub new_password: String,
}

/// One logged-in device of the current user.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SessionInfo {
    pub id: Id,
    pub created_at: i64,
    pub last_seen_at: i64,
    pub expires_at: i64,
    pub user_agent: Option<String>,
    /// The session this request was made with.
    pub current: bool,
}

// ---------------------------------------------------------------------------
// Users (admin)
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct UserSummary {
    pub id: Id,
    pub username: String,
    pub display_name: String,
    pub role: Role,
    pub disabled: bool,
    /// `false` until the user has accepted their invite.
    pub has_password: bool,
    pub group_ids: Vec<Id>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CreateUser {
    pub username: String,
    pub display_name: String,
    pub role: Role,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct UpdateUser {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<Role>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
}

/// A one-time link token. The client builds the URL `/invite/<token>`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Invite {
    pub user_id: Id,
    pub token: String,
    pub expires_at: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CreatedUser {
    pub user: UserSummary,
    pub invite: Invite,
}

// ---------------------------------------------------------------------------
// Instruments and groups
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Instrument {
    pub id: Id,
    pub name: String,
    pub family: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct InstrumentInput {
    pub name: String,
    #[serde(default)]
    pub family: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Group {
    pub id: Id,
    pub name: String,
    pub member_ids: Vec<Id>,
    /// Members may read every part tagged with one of these instruments.
    pub instrument_ids: Vec<Id>,
    /// Members may read every part of these scores.
    pub score_ids: Vec<Id>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GroupInput {
    pub name: String,
}

/// Replaces a whole list (members, grants, programme items).
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct IdList {
    pub ids: Vec<Id>,
}

// ---------------------------------------------------------------------------
// Library
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct ScoreInput {
    pub title: String,
    #[serde(default)]
    pub subtitle: Option<String>,
    #[serde(default)]
    pub composer: Option<String>,
    #[serde(default)]
    pub arranger: Option<String>,
    /// e.g. march, polka, concert, sacred, pop.
    #[serde(default)]
    pub genre: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub notes: Option<String>,
    /// Number of the piece in the paper archive, free text ("0304", "A-12").
    /// Blank = none. Not unique (a-archive-number).
    #[serde(default, deserialize_with = "blank_is_none")]
    pub archive_number: Option<String>,
}

/// A score with the parts the requesting user may read.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Score {
    pub id: Id,
    pub title: String,
    pub subtitle: Option<String>,
    pub composer: Option<String>,
    pub arranger: Option<String>,
    pub genre: Option<String>,
    pub tags: Vec<String>,
    pub notes: Option<String>,
    /// Number in the paper archive (see [`ScoreInput::archive_number`]).
    #[serde(default)]
    pub archive_number: Option<String>,
    pub parts: Vec<Part>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct PartInput {
    /// Custom name, e.g. "Solo Cornet". Empty or missing: the name is
    /// derived from instruments, key and voices (`naming::part_name`).
    #[serde(default, deserialize_with = "blank_is_none")]
    pub label: Option<String>,
    pub instrument_ids: Vec<Id>,
    /// Voices / chairs the part contains, in order, each free text: `["2"]`
    /// for "2nd Trumpet", `["1", "2"]` for "Trumpet 1 & 2", also "1a",
    /// "Solo". A JSON number is accepted per entry. Cleaned on input with
    /// `naming::clean_voices` (trimmed, split at "&", empty entries and
    /// duplicates ignoring case dropped, order kept).
    #[serde(default, deserialize_with = "voice_list")]
    pub voices: Vec<String>,
    /// Key of a transposing instrument: a code of `naming::KEYS` ("Bb",
    /// "Eb", "F", …); common spellings ("B-flat", "in Es") are normalised.
    /// `None` = concert pitch.
    #[serde(default, deserialize_with = "blank_is_none")]
    pub transposition: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Part {
    pub id: Id,
    pub score_id: Id,
    /// Custom name, `None` when the name is derived.
    pub label: Option<String>,
    /// The name to show everywhere: `label` or the derived name.
    pub display_name: String,
    pub instrument_ids: Vec<Id>,
    /// Voices in their stored order (table `part_voices`), possibly empty.
    pub voices: Vec<String>,
    /// Key code (see `naming::KEYS`); older data may hold other text.
    pub transposition: Option<String>,
    pub notes: Option<String>,
    /// The current file version, `None` until a file is uploaded.
    pub file: Option<FileVersion>,
}

fn blank_is_none<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    let v: Option<String> = Option::deserialize(d)?;
    Ok(v.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()))
}

fn voice_list<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum TextOrNumber {
        Text(String),
        Number(i64),
    }
    let v: Option<Vec<TextOrNumber>> = Option::deserialize(d)?;
    let raw: Vec<String> = v
        .unwrap_or_default()
        .into_iter()
        .map(|e| match e {
            TextOrNumber::Text(s) => s,
            TextOrNumber::Number(n) => n.to_string(),
        })
        .collect();
    Ok(crate::naming::clean_voices(&raw))
}

/// One uploaded file version of a part. Files are immutable and addressed
/// by their SHA-256; `GET /api/files/{sha256}` serves the bytes.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct FileVersion {
    pub version: i64,
    pub sha256: String,
    pub mime: String,
    pub size_bytes: i64,
    pub page_count: Option<i64>,
    pub has_thumbnail: bool,
    pub uploaded_at: i64,
}

// ---------------------------------------------------------------------------
// Concert programmes
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct ProgrammeInput {
    pub title: String,
    /// `YYYY-MM-DD`.
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ProgrammeSummary {
    pub id: Id,
    pub title: String,
    pub date: Option<String>,
    pub notes: Option<String>,
    /// Number of pieces the requesting user has at least one part for.
    pub item_count: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Programme {
    pub id: Id,
    pub title: String,
    pub date: Option<String>,
    pub notes: Option<String>,
    /// Pieces in concert order; only those the user may read.
    pub items: Vec<ProgrammeItem>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ProgrammeItem {
    pub position: i64,
    pub score: Score,
}

// ---------------------------------------------------------------------------
// Annotations
// ---------------------------------------------------------------------------

/// A named, private set of annotations of one user on one file version of a
/// part, e.g. "Christmas concert 2026".
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct AnnotationSet {
    pub id: Id,
    pub part_id: Id,
    /// The file version the annotations were drawn on.
    pub file_sha256: String,
    pub file_version: i64,
    pub name: String,
    pub programme_id: Option<Id>,
    pub is_default: bool,
    pub created_at: i64,
    pub updated_at: i64,
    /// Pages that have a saved layer, with their current revision.
    pub pages: Vec<PageRevision>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageRevision {
    /// Zero-based page index.
    pub page: i64,
    pub revision: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct NewAnnotationSet {
    pub name: String,
    #[serde(default)]
    pub programme_id: Option<Id>,
    /// Duplicate the pages of this set (same user and part) into the new one.
    #[serde(default)]
    pub copy_from: Option<Id>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct UpdateAnnotationSet {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// `Some(None)` (JSON `null`) unlinks the programme; absent leaves it.
    #[serde(default, deserialize_with = "double_option", skip_serializing_if = "Option::is_none")]
    pub programme_id: Option<Option<Id>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_default: Option<bool>,
}

/// Response of a successful page save (`PUT .../pages/{page}`).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct SavedPage {
    pub revision: i64,
}

/// Body of a 409 response to a page save: someone (another device of the
/// same user) saved in between. The client merges and retries.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageConflict {
    pub current_revision: i64,
}

// ---------------------------------------------------------------------------
// Mass import (librarians; F-LIB-004). Parsing and review rules: `crate::import`.
// ---------------------------------------------------------------------------

/// `POST /api/import/batches`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct NewImportBatch {
    /// Name of the selected folder.
    pub name: String,
    pub pattern: String,
    pub language: crate::import::Language,
}

/// `PUT /api/import/batches/{id}`: absent fields stay. A new pattern or
/// language reads the file names again (changes to pieces whose grouping
/// changes are lost; changes to parts are kept by path).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct UpdateImportBatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<crate::import::Language>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct ImportFileCounts {
    pub total: i64,
    pub pending: i64,
    pub uploaded: i64,
    pub duplicate: i64,
    pub failed: i64,
    /// Files the pattern does not match (not part of any piece).
    pub unmatched: i64,
}

/// An import batch (without its rows).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ImportBatch {
    pub id: Id,
    pub name: String,
    pub pattern: String,
    pub language: crate::import::Language,
    pub created_at: i64,
    pub created_by: Option<String>,
    pub files: ImportFileCounts,
    pub pieces: i64,
    pub committed: i64,
}

/// One file of the selected folder, as checked by the browser.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ImportFileInput {
    /// Path below the selected folder, `/`-separated.
    pub path: String,
    pub size_bytes: i64,
    /// SHA-256 of the content, lower-case hex.
    pub sha256: String,
}

/// `POST /api/import/batches/{id}/files`: the file list (all of it, or
/// again after an interruption).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct RegisterImportFiles {
    pub files: Vec<ImportFileInput>,
}

/// A file the server still needs: `PUT /api/import/files/{id}/content`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ImportUpload {
    pub id: Id,
    pub path: String,
}

/// Answer to [`RegisterImportFiles`].
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct RegisteredImportFiles {
    /// Files to upload (content not on the server yet).
    pub upload: Vec<ImportUpload>,
    pub files: ImportFileCounts,
}

/// `GET /api/import/batches/{id}`: everything the review shows.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ImportReview {
    pub batch: ImportBatch,
    pub pieces: Vec<crate::import::review::ReviewPiece>,
    /// Files the pattern does not match.
    pub unmatched: Vec<crate::import::review::StagedFile>,
    /// Files skipped because their content is already in the library or the batch.
    pub duplicates: Vec<crate::import::review::StagedFile>,
}

/// `PATCH /api/import/items`: changes of the librarian, merged into the
/// stored ones (fields that are `None` stay). `reset` first drops all
/// earlier changes of these items. Piece changes apply to piece ids, part
/// changes to part ids (several at once = bulk edit).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct UpdateImportItems {
    pub ids: Vec<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub piece: Option<crate::import::review::PieceOverrides>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part: Option<crate::import::review::PartOverrides>,
    #[serde(default)]
    pub reset: bool,
}

/// `POST /api/import/batches/{id}/commit`: these pieces (each must not be
/// red), or with `None` every green piece.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct CommitImport {
    #[serde(default)]
    pub piece_ids: Option<Vec<Id>>,
}

/// A piece the commit did not create, and why:
/// `{"id": 7, "code": "…", "args": {…}, "message": "…"}`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ImportItemError {
    pub id: Id,
    #[serde(flatten)]
    pub code: ErrorCode,
    /// English, for API users only: `code.to_string()`.
    pub message: String,
}

impl ImportItemError {
    pub fn new(id: Id, code: ErrorCode) -> Self {
        ImportItemError { id, message: code.to_string(), code }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct CommittedImport {
    /// Piece items committed now.
    pub committed: Vec<Id>,
    pub errors: Vec<ImportItemError>,
}

/// Answer to `POST /api/import/batches/{id}/undo`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct UndoneImport {
    /// Pieces deleted (created by the batch).
    pub pieces: i64,
    /// Parts deleted (incl. parts added to existing pieces).
    pub parts: i64,
}

/// A stored instrument alias (`GET /api/import/aliases`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct InstrumentAlias {
    /// Folded word(s) as found in file names, e.g. "klar".
    pub alias: String,
    pub instrument_id: Id,
    pub instrument_name: String,
}

/// `PUT /api/import/aliases`: map a file-name word to an instrument.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct InstrumentAliasInput {
    pub alias: String,
    pub instrument_id: Id,
}

fn double_option<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(d).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn programme_link_distinguishes_absent_and_null() {
        let absent: UpdateAnnotationSet = serde_json::from_str("{}").unwrap();
        assert_eq!(absent.programme_id, None);
        let null: UpdateAnnotationSet = serde_json::from_str(r#"{"programme_id":null}"#).unwrap();
        assert_eq!(null.programme_id, Some(None));
        let set: UpdateAnnotationSet = serde_json::from_str(r#"{"programme_id":7}"#).unwrap();
        assert_eq!(set.programme_id, Some(Some(7)));
        assert_eq!(serde_json::to_string(&null).unwrap(), r#"{"programme_id":null}"#);
    }

    #[test]
    fn part_input_accepts_blank_label_and_numeric_voices() {
        let p: PartInput =
            serde_json::from_str(r#"{"label":"  ","instrument_ids":[1],"voices":[2],"transposition":""}"#).unwrap();
        assert_eq!((p.label, p.voices, p.transposition), (None, vec!["2".to_owned()], None));
        let p: PartInput =
            serde_json::from_str(r#"{"instrument_ids":[],"voices":[" 1a ", 2, "", "1A", "2"]}"#).unwrap();
        assert_eq!((p.label, p.voices), (None, vec!["1a".to_owned(), "2".to_owned()]));
        let p: PartInput = serde_json::from_str(r#"{"instrument_ids":[],"voices":null}"#).unwrap();
        assert!(p.voices.is_empty());
        let p: PartInput = serde_json::from_str(r#"{"instrument_ids":[]}"#).unwrap();
        assert!(p.voices.is_empty());
        assert!(serde_json::from_str::<PartInput>(r#"{"instrument_ids":[],"voices":"1"}"#).is_err(), "a list");
    }

    #[test]
    fn score_input_trims_the_archive_number() {
        let s: ScoreInput = serde_json::from_str(r#"{"title":"x","archive_number":" 0304 "}"#).unwrap();
        assert_eq!(s.archive_number.as_deref(), Some("0304"));
        let s: ScoreInput = serde_json::from_str(r#"{"title":"x","archive_number":"  "}"#).unwrap();
        assert_eq!(s.archive_number, None);
        let s: ScoreInput = serde_json::from_str(r#"{"title":"x"}"#).unwrap();
        assert_eq!(s.archive_number, None);
    }

    #[test]
    fn roles_round_trip() {
        for role in [Role::Member, Role::Librarian, Role::Admin] {
            assert_eq!(Role::parse(role.as_str()), Some(role));
            let json = serde_json::to_string(&role).unwrap();
            assert_eq!(json, format!("\"{}\"", role.as_str()));
        }
        assert!(Role::Admin.can_manage_library() && !Role::Member.can_manage_library());
    }
}
