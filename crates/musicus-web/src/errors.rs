//! The one place that turns error codes and import notes into text
//! (a-error-codes, R-I18N-004). The server sends codes with arguments, never
//! sentences; every match here is exhaustive, so a new code, note or pattern
//! error does not compile until it has a text. English for now: each arm is
//! one message, to be replaced by a catalogue lookup (`error-<code>`) later.

use musicus_api::{
    ApiError, ErrorCode, Field,
    import::{Note, pattern::PatternError},
    ink::{InkError, InkLimit, MAX_ENCODED_BYTES},
};

/// Text of a failed request: the code's text, or the server's English
/// `message` for a code this build does not know (newer server).
pub fn api_error_text(e: &ApiError) -> String {
    match e.code {
        ErrorCode::Unknown => capitalise(&e.message),
        ref code => code_text(code),
    }
}

/// An error body this build could not parse as [`ApiError`] (a newer server
/// with an unknown code that has arguments): show its `message`.
pub fn unknown_error_text(message: &str) -> String {
    capitalise(message)
}

pub fn network_text(detail: &str) -> String {
    format!("Network error: {detail}")
}

pub fn http_failed_text(status: u16) -> String {
    format!("Request failed (HTTP {status})")
}

/// The sentence for an error code (also used for a staged file's problem).
pub fn code_text(code: &ErrorCode) -> String {
    use ErrorCode::*;
    match code {
        NotFound => "Not found".into(),
        Unauthorized => "Please log in".into(),
        Internal => "Something went wrong on the server. Please try again later".into(),
        PayloadTooLarge { max_bytes } => format!("The file is too large (at most {})", size_text(*max_bytes)),
        TooManyAttempts { minutes } => format!("Too many attempts. Try again in {minutes} minutes"),
        CsrfMissing => "The request was refused (missing security header). Reload the page".into(),
        LibrarianOnly => "Only librarians may do this".into(),
        AdminOnly => "Only admins may do this".into(),
        AlreadyExists => "An entry with this name already exists".into(),
        InUseOrUnknown => "The entry is still in use or refers to something that does not exist".into(),
        FieldEmpty { field } => format!("{} must not be empty", field_name(*field)),
        FieldTooLong { field, max } => format!("{} is longer than {max} characters", field_name(*field)),
        WrongPassword => "The current password is wrong".into(),
        PasswordTooShort { min } => format!("The password needs at least {min} characters"),
        PasswordTooLong { max } => format!("The password may have at most {max} characters"),
        UsernameInvalid => "Usernames may contain letters, digits, '.', '_' and '-'".into(),
        CannotLockOutSelf => "You cannot disable yourself or remove your own admin role".into(),
        UserDisabled => "Enable the user before inviting them".into(),
        TooManyTags { max } => format!("At most {max} tags"),
        TooManyInstruments { max } => format!("At most {max} instruments per part"),
        UnknownKey { key } => format!("Unknown key “{key}”: choose one from the list"),
        TooManyVoices { max } => format!("At most {max} voices per part"),
        VoiceTooLong { voice, max } => format!("The voice “{voice}” is longer than {max} characters"),
        UploadInterrupted => "The upload was interrupted. Please try again".into(),
        NotPdf => "Only PDF files are supported".into(),
        SystemsNotDetected => "The systems of this file have not been detected yet".into(),
        CorrectionTooManyBands { max } => format!("At most {max} bands per page"),
        CorrectionBoundsCount | CorrectionBoundsRange | CorrectionBoundsOrder => {
            "The corrected bands are invalid. Reload the page and try again".into()
        }
        UnknownProgramme => "Unknown programme".into(),
        CopyFromOtherPart => "The set to copy belongs to another part".into(),
        PartHasNoFile => "This part has no file yet".into(),
        PageCountChanged => "The new file version has a different page count".into(),
        IfMatchRequired | IfMatchInvalid => "The annotations could not be saved (revision missing)".into(),
        InvalidInk { reason } => ink_error_text(reason),
        DateFormat => "The date must look like 2026-12-24".into(),
        TooManyPieces { max } => format!("At most {max} pieces per programme"),
        InvalidPattern { reason } => capitalise(&pattern_error_text(reason)),
        InvalidPath { path } => format!("Invalid path “{path}”"),
        NotPdfName { path } => format!("Not a PDF file name: “{path}”"),
        TooManyFiles { max } => format!("At most {max} files per request"),
        InvalidFileHash { path } => format!("Invalid hash or size for “{path}”"),
        FileChanged => "The file changed after it was checked; select the folder again".into(),
        ItemCount { max } => format!("Choose 1 to {max} items"),
        UnknownItems => "Unknown items, or items of several batches".into(),
        ImportCommitted => "Already imported; undo the batch to change it".into(),
        UnknownScore => "Unknown piece".into(),
        UnknownInstruments => "Unknown instruments".into(),
        UnknownInstrument => "Unknown instrument".into(),
        FileNotUploaded { path } => format!("{path} is not uploaded"),
        UndoAnnotated { pieces } => {
            format!("Cannot undo: musicians have annotated imported parts of {}", pieces.join(", "))
        }
        UndoChanged { pieces } => format!(
            "Cannot undo: these imported pieces got other parts or are in a programme since: {}",
            pieces.join(", ")
        ),
        NotInBatch => "Not a piece of this batch".into(),
        DuplicateInLibrary { title } => format!("Already in the library: {title}"),
        DuplicateInBatch { path } => format!("Same file as {path}"),
        ImportBlocked { note: Some(note) } => capitalise(&note_text(note)),
        ImportBlocked { note: None } => "Not ready".into(),
        Unknown => "Unknown error".into(),
    }
}

/// Field names as the forms label them.
pub fn field_name(field: Field) -> &'static str {
    match field {
        Field::Name => "Name",
        Field::Family => "Family",
        Field::Username => "Username",
        Field::DisplayName => "Display name",
        Field::Title => "Title",
        Field::Subtitle => "Subtitle",
        Field::Composer => "Composer",
        Field::Arranger => "Arranger",
        Field::Genre => "Genre",
        Field::Notes => "Notes",
        Field::ArchiveNumber => "Archive number",
        Field::Tag => "A tag",
        Field::Label => "Custom name",
        Field::Date => "Date",
        Field::Pattern => "Pattern",
        Field::InstrumentWord => "An instrument word",
        Field::Alias => "The alias",
    }
}

/// Why an import value is yellow or red (shown in the review, lower case
/// because it often follows other text).
pub fn note_text(note: &Note) -> String {
    match note {
        Note::AmbiguousKey(k) => format!(
            "\"{k}\" is a key name in German only (B = B♭, Es = E♭, H = B♮): choose the key, or set the batch language to German"
        ),
        Note::UnknownKey(k) => format!("\"{k}\" is not a known key"),
        Note::InvalidKey(k) => format!("unknown key \"{k}\""),
        Note::TwoKeys(k) => format!("several keys in the name: {k}"),
        Note::SeveralVoices(v) => {
            format!("numbers without a separator in the name ({v}): check the voices (also a copy or page number?)")
        }
        Note::Abbreviation(t) => format!("\"{t}\" is an abbreviation: check the instrument"),
        Note::Guessed(t) => format!("\"{t}\" was guessed from the beginning of the word: check the instrument"),
        Note::AmbiguousInstrument(t) => format!("\"{t}\" can mean several instruments: check it"),
        Note::UnknownInstrument(t) => format!("unknown instrument \"{t}\": choose an instrument or create it"),
        Note::NoInstrument => "no instrument: nobody could get this part through an instrument grant".into(),
        Note::Unrecognised(t) => format!("not understood: \"{t}\""),
        Note::Contradictory(v) => format!("the files of this piece disagree: {v}"),
        Note::TitleMissing => "the file names contain no title: type it".into(),
        Note::SimilarTitle(t) => format!("a piece with a similar title exists: {t}"),
        Note::ConfirmTarget(t) => {
            format!("this may be the existing piece {t}: add the parts to it or import as a new piece")
        }
        Note::TargetGone => "the chosen existing piece was deleted".into(),
        Note::NoParts => "no parts to import".into(),
        Note::NotUploaded => "file not uploaded yet: select the folder again to continue".into(),
        Note::UploadFailed(Some(why)) => format!("upload failed: {}", code_text(why)),
        Note::UploadFailed(None) => "upload failed".into(),
        Note::ScoreWithOneStaff => "named as full score, but its systems have only one staff".into(),
        Note::ManyStaves(n) => format!("a single part, but its systems have {n} staves: a score?"),
        Note::NoReaders => "no group can read this part yet".into(),
    }
}

/// Why a file name pattern cannot be used (under the pattern field).
pub fn pattern_error_text(e: &PatternError) -> String {
    match e {
        PatternError::UnknownPlaceholder(p) => format!(
            "unknown placeholder {{{p}}}; use {{number}}, {{title}}, {{composer}}, {{arranger}}, {{genre}}, {{part}} or {{*}}"
        ),
        PatternError::Unbalanced => "brackets [ ] or braces { } do not match".into(),
        PatternError::MissingPart => "the pattern needs {part}".into(),
        PatternError::MissingTitleOrNumber => "the pattern needs {title} or {number}".into(),
        PatternError::Twice(p) => format!("{{{p}}} may appear only once"),
        PatternError::TooManyOptionals => {
            format!("at most {} optional groups [ ]", musicus_api::import::pattern::MAX_OPTIONALS)
        }
    }
}

/// Why an annotation page was rejected or could not be read.
pub fn ink_error_text(e: &InkError) -> String {
    match e {
        InkError::Empty | InkError::Malformed | InkError::DuplicateId(_) | InkError::PressureMismatch => {
            "The annotations of this page are damaged".into()
        }
        InkError::UnsupportedVersion(_) => "The annotations were saved by a newer version of Musicus".into(),
        InkError::TooLarge => {
            format!("The annotations of this page are too large (at most {})", size_text(MAX_ENCODED_BYTES as u64))
        }
        InkError::LimitExceeded(InkLimit::ObjectsPerPage) => {
            format!("Too many annotations on this page (at most {})", InkLimit::ObjectsPerPage.max())
        }
        InkError::LimitExceeded(InkLimit::PointsPerStroke) => {
            format!("A stroke is too long (at most {} points)", InkLimit::PointsPerStroke.max())
        }
        InkError::LimitExceeded(InkLimit::TextLength) => {
            format!("A text is too long (at most {} characters)", InkLimit::TextLength.max())
        }
    }
}

fn size_text(bytes: u64) -> String {
    const MB: u64 = 1024 * 1024;
    if bytes >= MB { format!("{} MB", bytes / MB) } else { format!("{} KB", bytes.div_ceil(1024)) }
}

fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every code has a text (`examples()` holds one of each, checked in musicus-api).
    #[test]
    fn every_code_has_a_sentence() {
        for code in musicus_api::error::examples() {
            let text = code_text(&code);
            assert!(text.chars().next().is_some_and(char::is_uppercase), "{code:?}: {text}");
            assert!(!text.contains("{") || matches!(code, ErrorCode::InvalidPattern { .. }), "{code:?}: {text}");
        }
    }

    #[test]
    fn arguments_appear_in_the_text() {
        assert_eq!(code_text(&ErrorCode::PasswordTooShort { min: 8 }), "The password needs at least 8 characters");
        assert_eq!(
            code_text(&ErrorCode::FieldTooLong { field: Field::DisplayName, max: 100 }),
            "Display name is longer than 100 characters"
        );
        assert_eq!(
            code_text(&ErrorCode::PayloadTooLarge { max_bytes: 64 * 1024 * 1024 }),
            "The file is too large (at most 64 MB)"
        );
        assert!(note_text(&Note::AmbiguousKey("B".into())).contains("German"));
        let failed = Note::UploadFailed(Some(Box::new(ErrorCode::NotPdf)));
        assert_eq!(note_text(&failed), "upload failed: Only PDF files are supported");
    }

    #[test]
    fn unknown_codes_show_the_server_message() {
        let e = ApiError { error: "conflict".into(), code: ErrorCode::Unknown, message: "something new".into() };
        assert_eq!(api_error_text(&e), "Something new");
        let known = ApiError::new("not_found", ErrorCode::NotFound);
        assert_eq!(api_error_text(&known), "Not found");
    }
}
