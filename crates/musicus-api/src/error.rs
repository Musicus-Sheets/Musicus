//! Error codes of the API (a-error-codes, R-I18N-004).
//!
//! The server sends no text for users: every error is an [`ErrorCode`] with
//! typed arguments, and the client turns it into a sentence in the user's
//! language. The English [`std::fmt::Display`] text below is only for API
//! users, logs and tests (the `message` of [`crate::ApiError`]).
//!
//! Wire format: adjacently tagged, flattened into the error body:
//! `{"error":"bad_request","code":"password_too_short","args":{"min":8},"message":"…"}`.
//! Codes without arguments have no `args`. Codes are stable: never rename or
//! reuse one; a changed meaning is a new code.

use crate::import::{Note, pattern::PatternError};
use crate::ink::InkError;
use serde::{Deserialize, Serialize};
use std::fmt;

/// A form field named in [`ErrorCode::FieldEmpty`] and [`ErrorCode::FieldTooLong`].
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Field {
    Name,
    Family,
    Username,
    DisplayName,
    Title,
    Subtitle,
    Composer,
    Arranger,
    Genre,
    Notes,
    ArchiveNumber,
    Tag,
    /// The custom name of a part.
    Label,
    Date,
    /// The file name pattern of an import batch.
    Pattern,
    /// An instrument word of an import file name.
    InstrumentWord,
    Alias,
}

impl Field {
    /// The serialised name (`display_name`), also used in English messages.
    pub fn as_str(self) -> &'static str {
        match self {
            Field::Name => "name",
            Field::Family => "family",
            Field::Username => "username",
            Field::DisplayName => "display_name",
            Field::Title => "title",
            Field::Subtitle => "subtitle",
            Field::Composer => "composer",
            Field::Arranger => "arranger",
            Field::Genre => "genre",
            Field::Notes => "notes",
            Field::ArchiveNumber => "archive_number",
            Field::Tag => "tag",
            Field::Label => "label",
            Field::Date => "date",
            Field::Pattern => "pattern",
            Field::InstrumentWord => "instrument_word",
            Field::Alias => "alias",
        }
    }
}

/// Why a request failed. Serialised as `{"code": snake_case, "args": {…}}`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "code", content = "args", rename_all = "snake_case")]
pub enum ErrorCode {
    // --- general -----------------------------------------------------------
    /// 404: missing, or not readable by the requester (indistinguishable, R-ACL-002).
    NotFound,
    /// 401: no valid session, or wrong username/password at login.
    Unauthorized,
    /// 500: details are only in the server log (R-SEC-008).
    Internal,
    PayloadTooLarge {
        max_bytes: u64,
    },
    TooManyAttempts {
        minutes: u64,
    },
    CsrfMissing,
    LibrarianOnly,
    AdminOnly,
    /// A unique constraint (name, alias, …) failed.
    AlreadyExists,
    /// A foreign key failed: still in use (delete) or referring to something missing.
    InUseOrUnknown,
    FieldEmpty {
        field: Field,
    },
    FieldTooLong {
        field: Field,
        max: u32,
    },
    /// 503: the server is starting (snapshot, database updates, checks); try
    /// again shortly (R-OPS-045).
    ServerStarting,
    /// 503: the server is shutting down and accepts no new work (R-OPS-043).
    ServerShuttingDown,
    // --- accounts ----------------------------------------------------------
    WrongPassword,
    PasswordTooShort {
        min: u32,
    },
    PasswordTooLong {
        max: u32,
    },
    UsernameInvalid,
    /// An admin tried to disable themselves or drop their own admin role.
    CannotLockOutSelf,
    /// An invite for a disabled user.
    UserDisabled,
    // --- library -----------------------------------------------------------
    TooManyTags {
        max: u32,
    },
    TooManyInstruments {
        max: u32,
    },
    UnknownKey {
        key: String,
    },
    TooManyVoices {
        max: u32,
    },
    VoiceTooLong {
        voice: String,
        max: u32,
    },
    UploadInterrupted,
    NotPdf,
    // --- system layout corrections -----------------------------------------
    SystemsNotDetected,
    CorrectionTooManyBands {
        max: u32,
    },
    CorrectionBoundsCount,
    CorrectionBoundsRange,
    CorrectionBoundsOrder,
    // --- annotations -------------------------------------------------------
    UnknownProgramme,
    CopyFromOtherPart,
    PartHasNoFile,
    PageCountChanged,
    IfMatchRequired,
    IfMatchInvalid,
    InvalidInk {
        reason: InkError,
    },
    // --- programmes --------------------------------------------------------
    DateFormat,
    TooManyPieces {
        max: u32,
    },
    // --- mass import -------------------------------------------------------
    InvalidPattern {
        reason: PatternError,
    },
    InvalidPath {
        path: String,
    },
    NotPdfName {
        path: String,
    },
    TooManyFiles {
        max: u32,
    },
    InvalidFileHash {
        path: String,
    },
    /// The uploaded bytes differ from the hash registered for the file.
    FileChanged,
    ItemCount {
        max: u32,
    },
    UnknownItems,
    ImportCommitted,
    UnknownScore,
    UnknownInstruments,
    UnknownInstrument,
    FileNotUploaded {
        path: String,
    },
    /// Undo refused: musicians annotated parts of these pieces (titles).
    UndoAnnotated {
        pieces: Vec<String>,
    },
    /// Undo refused: these pieces got other parts or are in a programme since.
    UndoChanged {
        pieces: Vec<String>,
    },
    /// Commit: the piece id is not a piece of this batch.
    NotInBatch,
    /// A staged file skipped because its content is already in the library
    /// (title of the piece); stored per file, not an HTTP error.
    DuplicateInLibrary {
        title: String,
    },
    /// A staged file skipped because the batch has the same content at `path`.
    DuplicateInBatch {
        path: String,
    },
    /// Commit: the piece is not ready; the note that blocks it, if any.
    ImportBlocked {
        note: Option<Note>,
    },
    // --- forward compatibility ---------------------------------------------
    /// A code this build does not know (sent by a newer server); show the `message`.
    #[serde(other)]
    Unknown,
}

impl fmt::Display for ErrorCode {
    /// English for API users and logs; the web client renders its own text.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use ErrorCode::*;
        match self {
            NotFound => f.write_str("not found"),
            Unauthorized => f.write_str("login required"),
            Internal => f.write_str("internal error"),
            PayloadTooLarge { max_bytes } => write!(f, "request body larger than {max_bytes} bytes"),
            TooManyAttempts { minutes } => write!(f, "too many attempts, try again in {minutes} minutes"),
            CsrfMissing => f.write_str("missing CSRF header"),
            LibrarianOnly => f.write_str("only librarians may do this"),
            AdminOnly => f.write_str("only admins may do this"),
            AlreadyExists => f.write_str("an entry with this name already exists"),
            InUseOrUnknown => f.write_str("the entry is still in use or refers to something that does not exist"),
            FieldEmpty { field } => write!(f, "{} must not be empty", field.as_str()),
            FieldTooLong { field, max } => write!(f, "{} is longer than {max} characters", field.as_str()),
            ServerStarting => f.write_str("the server is starting, try again in a moment"),
            ServerShuttingDown => f.write_str("the server is shutting down, try again in a moment"),
            WrongPassword => f.write_str("the current password is wrong"),
            PasswordTooShort { min } => write!(f, "the password needs at least {min} characters"),
            PasswordTooLong { max } => write!(f, "the password may have at most {max} characters"),
            UsernameInvalid => f.write_str("usernames may contain letters, digits, '.', '_' and '-'"),
            CannotLockOutSelf => f.write_str("you cannot disable yourself or remove your own admin role"),
            UserDisabled => f.write_str("enable the user before inviting them"),
            TooManyTags { max } => write!(f, "at most {max} tags"),
            TooManyInstruments { max } => write!(f, "at most {max} instruments per part"),
            UnknownKey { key } => write!(f, "unknown key \"{key}\""),
            TooManyVoices { max } => write!(f, "at most {max} voices per part"),
            VoiceTooLong { voice, max } => write!(f, "voice \"{voice}\" is longer than {max} characters"),
            UploadInterrupted => f.write_str("upload interrupted"),
            NotPdf => f.write_str("only PDF files are supported"),
            SystemsNotDetected => f.write_str("the systems of this file have not been detected yet"),
            CorrectionTooManyBands { max } => write!(f, "at most {max} bands per page"),
            CorrectionBoundsCount => f.write_str("bounds must have one entry more than kinds"),
            CorrectionBoundsRange => f.write_str("bounds must lie between 0 and 1"),
            CorrectionBoundsOrder => f.write_str("bounds must increase"),
            UnknownProgramme => f.write_str("unknown programme"),
            CopyFromOtherPart => f.write_str("the set to copy belongs to another part"),
            PartHasNoFile => f.write_str("this part has no file yet"),
            PageCountChanged => f.write_str("the new file version has a different page count"),
            IfMatchRequired => f.write_str("send If-Match with the revision you edited (\"0\" for a new page)"),
            IfMatchInvalid => f.write_str("If-Match must be a revision number"),
            InvalidInk { reason } => write!(f, "{reason}"),
            DateFormat => f.write_str("the date must look like 2026-12-24"),
            TooManyPieces { max } => write!(f, "at most {max} pieces per programme"),
            InvalidPattern { reason } => write!(f, "{reason}"),
            InvalidPath { path } => write!(f, "invalid path \"{path}\""),
            NotPdfName { path } => write!(f, "not a PDF file name: \"{path}\""),
            TooManyFiles { max } => write!(f, "at most {max} files per request"),
            InvalidFileHash { path } => write!(f, "invalid hash or size for \"{path}\""),
            FileChanged => f.write_str("the file changed after it was checked; select the folder again"),
            ItemCount { max } => write!(f, "1 to {max} items"),
            UnknownItems => f.write_str("unknown items, or items of several batches"),
            ImportCommitted => f.write_str("already imported; undo the batch to change it"),
            UnknownScore => f.write_str("unknown piece"),
            UnknownInstruments => f.write_str("unknown instruments"),
            UnknownInstrument => f.write_str("unknown instrument"),
            FileNotUploaded { path } => write!(f, "{path} is not uploaded"),
            UndoAnnotated { pieces } => {
                write!(f, "cannot undo: musicians have annotated imported parts of {}", pieces.join(", "))
            }
            UndoChanged { pieces } => write!(
                f,
                "cannot undo: these imported pieces got other parts or are in a programme since: {}",
                pieces.join(", ")
            ),
            NotInBatch => f.write_str("not a piece of this batch"),
            DuplicateInLibrary { title } => write!(f, "already in the library: {title}"),
            DuplicateInBatch { path } => write!(f, "same file as {path}"),
            ImportBlocked { note: None } => f.write_str("not ready"),
            ImportBlocked { note: Some(note) } => write!(f, "not ready: {}", note.kind()),
            Unknown => f.write_str("unknown error"),
        }
    }
}

/// One value of every code (with sample arguments), for tests that must
/// cover all codes, e.g. the client's texts. Keep it complete: the test
/// `examples_cover_every_code` fails otherwise.
#[doc(hidden)]
pub fn examples() -> Vec<ErrorCode> {
    use ErrorCode::*;
    vec![
        NotFound,
        Unauthorized,
        Internal,
        PayloadTooLarge { max_bytes: 1024 },
        TooManyAttempts { minutes: 15 },
        CsrfMissing,
        LibrarianOnly,
        AdminOnly,
        AlreadyExists,
        InUseOrUnknown,
        FieldEmpty { field: Field::Title },
        FieldTooLong { field: Field::DisplayName, max: 100 },
        ServerStarting,
        ServerShuttingDown,
        WrongPassword,
        PasswordTooShort { min: 8 },
        PasswordTooLong { max: 256 },
        UsernameInvalid,
        CannotLockOutSelf,
        UserDisabled,
        TooManyTags { max: 30 },
        TooManyInstruments { max: 20 },
        UnknownKey { key: "X".into() },
        TooManyVoices { max: 10 },
        VoiceTooLong { voice: "123456789012345678901".into(), max: 20 },
        UploadInterrupted,
        NotPdf,
        SystemsNotDetected,
        CorrectionTooManyBands { max: 60 },
        CorrectionBoundsCount,
        CorrectionBoundsRange,
        CorrectionBoundsOrder,
        UnknownProgramme,
        CopyFromOtherPart,
        PartHasNoFile,
        PageCountChanged,
        IfMatchRequired,
        IfMatchInvalid,
        InvalidInk { reason: InkError::TooLarge },
        DateFormat,
        TooManyPieces { max: 200 },
        InvalidPattern { reason: PatternError::MissingPart },
        InvalidPath { path: "../a.pdf".into() },
        NotPdfName { path: "a.txt".into() },
        TooManyFiles { max: 20_000 },
        InvalidFileHash { path: "a.pdf".into() },
        FileChanged,
        ItemCount { max: 5000 },
        UnknownItems,
        ImportCommitted,
        UnknownScore,
        UnknownInstruments,
        UnknownInstrument,
        FileNotUploaded { path: "March/Horn.pdf".into() },
        UndoAnnotated { pieces: vec!["March".into()] },
        UndoChanged { pieces: vec!["March".into(), "Waltz".into()] },
        NotInBatch,
        DuplicateInLibrary { title: "March".into() },
        DuplicateInBatch { path: "Other/Horn.pdf".into() },
        ImportBlocked { note: Some(Note::NoParts) },
        Unknown,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ApiError;
    use serde_json::json;

    /// Exhaustive (no wildcard): a new code does not compile until it gets an
    /// index here, and the test below then demands an example for it.
    fn index(c: &ErrorCode) -> usize {
        use ErrorCode::*;
        match c {
            NotFound => 0,
            Unauthorized => 1,
            Internal => 2,
            PayloadTooLarge { .. } => 3,
            TooManyAttempts { .. } => 4,
            CsrfMissing => 5,
            LibrarianOnly => 6,
            AdminOnly => 7,
            AlreadyExists => 8,
            InUseOrUnknown => 9,
            FieldEmpty { .. } => 10,
            FieldTooLong { .. } => 11,
            WrongPassword => 12,
            PasswordTooShort { .. } => 13,
            PasswordTooLong { .. } => 14,
            UsernameInvalid => 15,
            CannotLockOutSelf => 16,
            UserDisabled => 17,
            TooManyTags { .. } => 18,
            TooManyInstruments { .. } => 19,
            UnknownKey { .. } => 20,
            TooManyVoices { .. } => 21,
            VoiceTooLong { .. } => 22,
            UploadInterrupted => 23,
            NotPdf => 24,
            SystemsNotDetected => 25,
            CorrectionTooManyBands { .. } => 26,
            CorrectionBoundsCount => 27,
            CorrectionBoundsRange => 28,
            CorrectionBoundsOrder => 29,
            UnknownProgramme => 30,
            CopyFromOtherPart => 31,
            PartHasNoFile => 32,
            PageCountChanged => 33,
            IfMatchRequired => 34,
            IfMatchInvalid => 35,
            InvalidInk { .. } => 36,
            DateFormat => 37,
            TooManyPieces { .. } => 38,
            InvalidPattern { .. } => 39,
            InvalidPath { .. } => 40,
            NotPdfName { .. } => 41,
            TooManyFiles { .. } => 42,
            InvalidFileHash { .. } => 43,
            FileChanged => 44,
            ItemCount { .. } => 45,
            UnknownItems => 46,
            ImportCommitted => 47,
            UnknownScore => 48,
            UnknownInstruments => 49,
            UnknownInstrument => 50,
            FileNotUploaded { .. } => 51,
            UndoAnnotated { .. } => 52,
            UndoChanged { .. } => 53,
            NotInBatch => 54,
            DuplicateInLibrary { .. } => 55,
            DuplicateInBatch { .. } => 56,
            ImportBlocked { .. } => 57,
            Unknown => 58,
            ServerStarting => 59,
            ServerShuttingDown => 60,
        }
    }
    const CODES: usize = 61;

    #[test]
    fn examples_cover_every_code() {
        let mut seen: Vec<usize> = examples().iter().map(index).collect();
        seen.sort_unstable();
        assert_eq!(seen, (0..CODES).collect::<Vec<_>>(), "one example per code, in index order");
    }

    #[test]
    fn codes_are_unique_snake_case_and_round_trip() {
        let mut names = std::collections::HashSet::new();
        for code in examples() {
            let v = serde_json::to_value(&code).unwrap();
            let name = v["code"].as_str().unwrap().to_owned();
            assert!(name.chars().all(|c| c.is_ascii_lowercase() || c == '_'), "{name}");
            assert!(names.insert(name.clone()), "duplicate code {name}");
            assert_eq!(serde_json::from_value::<ErrorCode>(v).unwrap(), code, "{name}");
            assert!(!code.to_string().is_empty());
        }
    }

    #[test]
    fn wire_format() {
        let e = ApiError::new("bad_request", ErrorCode::PasswordTooShort { min: 8 });
        assert_eq!(
            serde_json::to_value(&e).unwrap(),
            json!({"error": "bad_request", "code": "password_too_short", "args": {"min": 8},
                   "message": "the password needs at least 8 characters"})
        );
        let unit = serde_json::to_value(ApiError::new("not_found", ErrorCode::NotFound)).unwrap();
        assert_eq!(unit, json!({"error": "not_found", "code": "not_found", "message": "not found"}));
        let field = serde_json::to_value(ErrorCode::FieldTooLong { field: Field::DisplayName, max: 100 }).unwrap();
        assert_eq!(field, json!({"code": "field_too_long", "args": {"field": "display_name", "max": 100}}));
        let note = serde_json::to_value(ErrorCode::ImportBlocked { note: Some(Note::UnknownInstrument("Xy".into())) });
        assert_eq!(
            note.unwrap(),
            json!({"code": "import_blocked", "args": {"note": {"kind": "unknown_instrument", "detail": "Xy"}}})
        );
        let back: ApiError = serde_json::from_value(serde_json::to_value(&e).unwrap()).unwrap();
        assert_eq!(back, e);
    }

    #[test]
    fn unknown_codes_from_a_newer_server() {
        // Without arguments: `Unknown`, and the client shows the English `message`.
        let e: ApiError =
            serde_json::from_value(json!({"error": "conflict", "code": "brand_new", "message": "something new"}))
                .unwrap();
        assert_eq!((e.code, e.message.as_str()), (ErrorCode::Unknown, "something new"));
        // With arguments serde cannot skip them: clients parse `{error, message}` instead.
        let with_args = json!({"error": "conflict", "code": "brand_new", "args": {"x": 1}, "message": "m"});
        assert!(serde_json::from_value::<ApiError>(with_args).is_err());
    }
}
