# Error codes from the server, import notes, pattern and annotation errors
# (a-error-codes, R-I18N-004). One message per code: `error-<code>`; ids never
# change meaning. Arguments that are user data (a path, a title, a voice) are
# shown as entered.

## Requests
# $detail: the browser's message.
error-network = Network error: { $detail }
error-http-failed = Request failed (HTTP { $status })
error-unknown = Unknown error

## Error codes
error-not-found = Not found
error-unauthorized = Please log in
error-internal = Something went wrong on the server. Please try again later
# $size: a formatted size ("64 MB").
error-payload-too-large = The file is too large (at most { $size })
error-too-many-attempts = Too many attempts. Try again in { $minutes ->
    [one] { $minutes } minute
   *[other] { $minutes } minutes
}
error-csrf-missing = The request was refused (missing security header). Reload the page
error-librarian-only = Only librarians may do this
error-admin-only = Only admins may do this
error-already-exists = An entry with this name already exists
error-in-use-or-unknown = The entry is still in use or refers to something that does not exist
# $field: a field name below (error-field-*).
error-field-empty = { $field } must not be empty
error-field-too-long = { $field } is longer than { $max } characters
# 503 while the server starts (snapshot, database updates) or after its stop signal.
error-server-starting = The server is starting. Try again in a moment
error-server-shutting-down = The server is shutting down. Try again when it is back
error-wrong-password = The current password is wrong
error-password-too-short = The password needs at least { $min } characters
error-password-too-long = The password may have at most { $max } characters
error-username-invalid = Usernames may contain letters, digits, '.', '_' and '-'
error-cannot-lock-out-self = You cannot disable yourself or remove your own admin role
error-user-disabled = Enable the user before inviting them
error-too-many-tags = At most { $max } tags
error-too-many-instruments = At most { $max } instruments per part
# $key: the key as typed.
error-unknown-key = Unknown key “{ $key }”: choose one from the list
error-too-many-voices = At most { $max } voices per part
error-voice-too-long = The voice “{ $voice }” is longer than { $max } characters
error-upload-interrupted = The upload was interrupted. Please try again
error-not-pdf = Only PDF files are supported
error-systems-not-detected = The systems of this file have not been detected yet
error-correction-too-many-bands = At most { $max } bands per page
# Bands of the system editor out of order or range.
error-correction-invalid = The corrected bands are invalid. Reload the page and try again
error-unknown-programme = Unknown programme
error-copy-from-other-part = The set to copy belongs to another part
error-part-has-no-file = This part has no file yet
error-page-count-changed = The new file version has a different page count
error-if-match = The annotations could not be saved (revision missing)
error-date-format = The date must look like 2026-12-24
error-too-many-pieces = At most { $max } pieces per programme
error-invalid-path = Invalid path “{ $path }”
error-not-pdf-name = Not a PDF file name: “{ $path }”
error-too-many-files = At most { $max } files per request
error-invalid-file-hash = Invalid hash or size for “{ $path }”
error-file-changed = The file changed after it was checked; select the folder again
error-item-count = Choose 1 to { $max } items
error-unknown-items = Unknown items, or items of several batches
error-import-committed = Already imported; undo the batch to change it
error-unknown-score = Unknown piece
error-unknown-instruments = Unknown instruments
error-unknown-instrument = Unknown instrument
error-file-not-uploaded = { $path } is not uploaded
# $pieces: titles, joined with commas.
error-undo-annotated = Cannot undo: musicians have annotated imported parts of { $pieces }
error-undo-changed = Cannot undo: these imported pieces got other parts or are in a programme since: { $pieces }
error-not-in-batch = Not a piece of this batch
error-duplicate-in-library = Already in the library: { $title }
error-duplicate-in-batch = Same file as { $path }
error-import-not-ready = Not ready

## Field names in error messages (as the forms label them)
error-field-name = Name
error-field-family = Family
error-field-username = Username
error-field-display-name = Display name
error-field-title = Title
error-field-subtitle = Subtitle
error-field-composer = Composer
error-field-arranger = Arranger
error-field-genre = Genre
error-field-notes = Notes
error-field-archive-number = Archive number
error-field-tag = A tag
error-field-label = Custom name
error-field-date = Date
error-field-pattern = Pattern
error-field-instrument-word = An instrument word
error-field-alias = The alias

## Import notes: why a value of the review is yellow or red. Lower case: they
## often follow other text (the client capitalises them when they stand alone).
note-ambiguous-key = "{ $key }" is a key name in German only (B = B♭, Es = E♭, H = B♮): choose the key, or set the batch language to German
note-unknown-key = "{ $key }" is not a known key
note-invalid-key = unknown key "{ $key }"
note-two-keys = several keys in the name: { $keys }
note-several-voices = numbers without a separator in the name ({ $voices }): check the voices (also a copy or page number?)
note-abbreviation = "{ $word }" is an abbreviation: check the instrument
note-guessed = "{ $word }" was guessed from the beginning of the word: check the instrument
note-ambiguous-instrument = "{ $word }" can mean several instruments: check it
note-unknown-instrument = unknown instrument "{ $word }": choose an instrument or create it
note-no-instrument = no instrument: nobody could get this part through an instrument grant
note-unrecognised = not understood: "{ $text }"
note-contradictory = the files of this piece disagree: { $values }
note-title-missing = the file names contain no title: type it
note-similar-title = a piece with a similar title exists: { $piece }
note-confirm-target = this may be the existing piece { $piece }: add the parts to it or import as a new piece
note-target-gone = the chosen existing piece was deleted
note-no-parts = no parts to import
note-not-uploaded = file not uploaded yet: select the folder again to continue
# $error: the reason (an error message above).
note-upload-failed-because = upload failed: { $error }
note-upload-failed = upload failed
note-score-with-one-staff = named as full score, but its systems have only one staff
note-many-staves = a single part, but its systems have { $count } staves: a score?
note-no-readers = no group can read this part yet

## File name patterns of the import (lower case, like the notes)
# $placeholder: the unknown one; $known: the valid ones. Never translate placeholders.
error-pattern-unknown-placeholder = unknown placeholder { $placeholder }; use { $known }
error-pattern-unbalanced = brackets [ ] or braces {"{"} {"}"} do not match
# $placeholder: "{part}" etc., never translated.
error-pattern-missing = the pattern needs { $placeholder }
error-pattern-missing-title-or-number = the pattern needs { $title } or { $number }
error-pattern-twice = { $placeholder } may appear only once
error-pattern-too-many-optionals = at most { $max } optional groups [ ]

## Annotation pages
error-ink-damaged = The annotations of this page are damaged
error-ink-newer-version = The annotations were saved by a newer version of Musicus
error-ink-too-large = The annotations of this page are too large (at most { $size })
error-ink-too-many-objects = Too many annotations on this page (at most { $max })
error-ink-stroke-too-long = A stroke is too long (at most { $max } points)
error-ink-text-too-long = A text is too long (at most { $max } characters)
