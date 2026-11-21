//! The one place that turns error codes and import notes into text
//! (a-error-codes, R-I18N-004). The server sends codes with arguments, never
//! sentences; every match here is exhaustive, so a new code, note or pattern
//! error does not compile until it has a message. The messages are in the
//! catalogue (`locales/en/errors.ftl`: `error-<code>`, `error-field-<field>`,
//! `note-<kind>`, `error-pattern-*`, `error-ink-*`; d-i18n-fluent).

use musicus_api::{
    ApiError, ErrorCode, Field,
    import::{Note, pattern::PatternError},
    ink::{InkError, InkLimit, MAX_ENCODED_BYTES},
};
use musicus_i18n::t;

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
    t::error_network(detail)
}

pub fn http_failed_text(status: u16) -> String {
    t::error_http_failed(status as u32)
}

/// The sentence for an error code (also used for a staged file's problem).
pub fn code_text(code: &ErrorCode) -> String {
    use ErrorCode::*;
    match code {
        NotFound => t::error_not_found(),
        Unauthorized => t::error_unauthorized(),
        Internal => t::error_internal(),
        PayloadTooLarge { max_bytes } => t::error_payload_too_large(size_text(*max_bytes)),
        TooManyAttempts { minutes } => t::error_too_many_attempts(*minutes),
        CsrfMissing => t::error_csrf_missing(),
        LibrarianOnly => t::error_librarian_only(),
        AdminOnly => t::error_admin_only(),
        AlreadyExists => t::error_already_exists(),
        InUseOrUnknown => t::error_in_use_or_unknown(),
        FieldEmpty { field } => t::error_field_empty(field_name(*field)),
        FieldTooLong { field, max } => t::error_field_too_long(field_name(*field), *max),
        WrongPassword => t::error_wrong_password(),
        PasswordTooShort { min } => t::error_password_too_short(*min),
        PasswordTooLong { max } => t::error_password_too_long(*max),
        UsernameInvalid => t::error_username_invalid(),
        CannotLockOutSelf => t::error_cannot_lock_out_self(),
        UserDisabled => t::error_user_disabled(),
        TooManyTags { max } => t::error_too_many_tags(*max),
        TooManyInstruments { max } => t::error_too_many_instruments(*max),
        UnknownKey { key } => t::error_unknown_key(key),
        TooManyVoices { max } => t::error_too_many_voices(*max),
        VoiceTooLong { voice, max } => t::error_voice_too_long(voice, *max),
        UploadInterrupted => t::error_upload_interrupted(),
        NotPdf => t::error_not_pdf(),
        SystemsNotDetected => t::error_systems_not_detected(),
        CorrectionTooManyBands { max } => t::error_correction_too_many_bands(*max),
        CorrectionBoundsCount | CorrectionBoundsRange | CorrectionBoundsOrder => t::error_correction_invalid(),
        UnknownProgramme => t::error_unknown_programme(),
        CopyFromOtherPart => t::error_copy_from_other_part(),
        PartHasNoFile => t::error_part_has_no_file(),
        PageCountChanged => t::error_page_count_changed(),
        IfMatchRequired | IfMatchInvalid => t::error_if_match(),
        InvalidInk { reason } => ink_error_text(reason),
        DateFormat => t::error_date_format(),
        TooManyPieces { max } => t::error_too_many_pieces(*max),
        InvalidPattern { reason } => capitalise(&pattern_error_text(reason)),
        InvalidPath { path } => t::error_invalid_path(path),
        NotPdfName { path } => t::error_not_pdf_name(path),
        TooManyFiles { max } => t::error_too_many_files(*max),
        InvalidFileHash { path } => t::error_invalid_file_hash(path),
        FileChanged => t::error_file_changed(),
        ItemCount { max } => t::error_item_count(*max),
        UnknownItems => t::error_unknown_items(),
        ImportCommitted => t::error_import_committed(),
        UnknownScore => t::error_unknown_score(),
        UnknownInstruments => t::error_unknown_instruments(),
        UnknownInstrument => t::error_unknown_instrument(),
        FileNotUploaded { path } => t::error_file_not_uploaded(path),
        UndoAnnotated { pieces } => t::error_undo_annotated(pieces.join(", ")),
        UndoChanged { pieces } => t::error_undo_changed(pieces.join(", ")),
        NotInBatch => t::error_not_in_batch(),
        DuplicateInLibrary { title } => t::error_duplicate_in_library(title),
        DuplicateInBatch { path } => t::error_duplicate_in_batch(path),
        ImportBlocked { note: Some(note) } => capitalise(&note_text(note)),
        ImportBlocked { note: None } => t::error_import_not_ready(),
        Unknown => t::error_unknown(),
    }
}

/// Field names as the forms label them.
pub fn field_name(field: Field) -> String {
    match field {
        Field::Name => t::error_field_name(),
        Field::Family => t::error_field_family(),
        Field::Username => t::error_field_username(),
        Field::DisplayName => t::error_field_display_name(),
        Field::Title => t::error_field_title(),
        Field::Subtitle => t::error_field_subtitle(),
        Field::Composer => t::error_field_composer(),
        Field::Arranger => t::error_field_arranger(),
        Field::Genre => t::error_field_genre(),
        Field::Notes => t::error_field_notes(),
        Field::ArchiveNumber => t::error_field_archive_number(),
        Field::Tag => t::error_field_tag(),
        Field::Label => t::error_field_label(),
        Field::Date => t::error_field_date(),
        Field::Pattern => t::error_field_pattern(),
        Field::InstrumentWord => t::error_field_instrument_word(),
        Field::Alias => t::error_field_alias(),
    }
}

/// Why an import value is yellow or red (shown in the review, lower case
/// because it often follows other text).
pub fn note_text(note: &Note) -> String {
    match note {
        Note::AmbiguousKey(k) => t::note_ambiguous_key(k),
        Note::UnknownKey(k) => t::note_unknown_key(k),
        Note::InvalidKey(k) => t::note_invalid_key(k),
        Note::TwoKeys(k) => t::note_two_keys(k),
        Note::SeveralVoices(v) => t::note_several_voices(v),
        Note::Abbreviation(w) => t::note_abbreviation(w),
        Note::Guessed(w) => t::note_guessed(w),
        Note::AmbiguousInstrument(w) => t::note_ambiguous_instrument(w),
        Note::UnknownInstrument(w) => t::note_unknown_instrument(w),
        Note::NoInstrument => t::note_no_instrument(),
        Note::Unrecognised(x) => t::note_unrecognised(x),
        Note::Contradictory(v) => t::note_contradictory(v),
        Note::TitleMissing => t::note_title_missing(),
        Note::SimilarTitle(p) => t::note_similar_title(p),
        Note::ConfirmTarget(p) => t::note_confirm_target(p),
        Note::TargetGone => t::note_target_gone(),
        Note::NoParts => t::note_no_parts(),
        Note::NotUploaded => t::note_not_uploaded(),
        Note::UploadFailed(Some(why)) => t::note_upload_failed_because(code_text(why)),
        Note::UploadFailed(None) => t::note_upload_failed(),
        Note::ScoreWithOneStaff => t::note_score_with_one_staff(),
        Note::ManyStaves(n) => t::note_many_staves(*n),
        Note::NoReaders => t::note_no_readers(),
    }
}

/// The placeholders of a pattern; never translated.
const PLACEHOLDERS: &str = "{number}, {title}, {composer}, {arranger}, {genre}, {part}, {*}";

/// Why a file name pattern cannot be used (under the pattern field).
pub fn pattern_error_text(e: &PatternError) -> String {
    let braced = |p: &str| format!("{{{p}}}");
    match e {
        PatternError::UnknownPlaceholder(p) => t::error_pattern_unknown_placeholder(braced(p), PLACEHOLDERS),
        PatternError::Unbalanced => t::error_pattern_unbalanced(),
        PatternError::MissingPart => t::error_pattern_missing("{part}"),
        PatternError::MissingTitleOrNumber => t::error_pattern_missing_title_or_number("{title}", "{number}"),
        PatternError::Twice(p) => t::error_pattern_twice(braced(p)),
        PatternError::TooManyOptionals => {
            t::error_pattern_too_many_optionals(musicus_api::import::pattern::MAX_OPTIONALS)
        }
    }
}

/// Why an annotation page was rejected or could not be read.
pub fn ink_error_text(e: &InkError) -> String {
    match e {
        InkError::Empty | InkError::Malformed | InkError::DuplicateId(_) | InkError::PressureMismatch => {
            t::error_ink_damaged()
        }
        InkError::UnsupportedVersion(_) => t::error_ink_newer_version(),
        InkError::TooLarge => t::error_ink_too_large(size_text(MAX_ENCODED_BYTES as u64)),
        InkError::LimitExceeded(l @ InkLimit::ObjectsPerPage) => t::error_ink_too_many_objects(l.max()),
        InkError::LimitExceeded(l @ InkLimit::PointsPerStroke) => t::error_ink_stroke_too_long(l.max()),
        InkError::LimitExceeded(l @ InkLimit::TextLength) => t::error_ink_text_too_long(l.max()),
    }
}

/// A limit in whole MB / KB (limits are round numbers; no `Intl` needed, so
/// this also runs in host tests).
fn size_text(bytes: u64) -> String {
    const MB: u64 = 1024 * 1024;
    if bytes >= MB { t::common_size_mb(bytes / MB) } else { t::common_size_kb(bytes.div_ceil(1024)) }
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

    /// In the pseudo-locale every text is marked: nothing English is left in
    /// this module (R-I18N-010). The language is per thread.
    #[test]
    fn every_text_comes_from_the_catalogue() {
        musicus_i18n::set_language(musicus_i18n::PSEUDO);
        let marked = |s: String| assert!(s.contains('⟦'), "not from the catalogue: {s}");
        for code in musicus_api::error::examples() {
            marked(code_text(&code));
        }
        for f in [Field::Name, Field::Tag, Field::Alias] {
            marked(field_name(f));
        }
        marked(note_text(&Note::ManyStaves(5)));
        marked(pattern_error_text(&PatternError::MissingPart));
        marked(ink_error_text(&InkError::TooLarge));
        marked(network_text("x"));
        marked(http_failed_text(500));
        musicus_i18n::set_language("en");
    }

    /// German texts (shipped): every code has a German sentence with its arguments.
    #[test]
    fn german_texts_for_codes_and_notes() {
        musicus_i18n::set_language("de");
        for code in musicus_api::error::examples() {
            let text = code_text(&code);
            assert!(text.chars().next().is_some_and(char::is_uppercase), "{code:?}: {text}");
        }
        assert_eq!(code_text(&ErrorCode::PasswordTooShort { min: 8 }), "Das Passwort braucht mindestens 8 Zeichen");
        assert_eq!(
            code_text(&ErrorCode::FieldTooLong { field: Field::DisplayName, max: 100 }),
            "Anzeigename ist länger als 100 Zeichen"
        );
        assert_eq!(
            code_text(&ErrorCode::TooManyAttempts { minutes: 1 }),
            "Zu viele Versuche. Versuch es in 1 Minute noch einmal"
        );
        let failed = Note::UploadFailed(Some(Box::new(ErrorCode::NotPdf)));
        assert_eq!(note_text(&failed), "Hochladen fehlgeschlagen: Nur PDF-Dateien werden unterstützt");
        musicus_i18n::set_language("en");
    }

    #[test]
    fn unknown_codes_show_the_server_message() {
        let e = ApiError { error: "conflict".into(), code: ErrorCode::Unknown, message: "something new".into() };
        assert_eq!(api_error_text(&e), "Something new");
        let known = ApiError::new("not_found", ErrorCode::NotFound);
        assert_eq!(api_error_text(&known), "Not found");
    }
}
