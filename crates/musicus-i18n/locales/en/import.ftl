# Mass import: batch list, new import with preview, review.

## Import page
import-title = Import
import-intro = Import a whole folder of PDF parts: Musicus reads number, title, composer and the parts from the folder and file names. Nothing reaches the library until you have checked and imported it.
import-earlier = Earlier imports
import-none = No imports yet.
import-forget-confirm = Forget this import? Pieces already imported stay in the library.
# $by: import-batch-by or nothing.
import-batch-line = { $time }{ $by } · { $pieces } pieces, { $committed } imported · { $files } files
import-batch-by = {" "}by { $name }
import-batch-missing = { $count } files not uploaded yet
import-review = Review
import-forget = Forget
import-select-folder = Select folder…
import-drop-folder = or drop the folder here
import-read-failed = Could not read { $path }: { $error }

## Progress of hashing and uploading
import-progress-hashing = Checking files… { $done } of { $total }
import-progress-registering = Comparing with the library…
import-progress-uploading = Uploading { $done } of { $total }{ $failed ->
    [0] {""}
   *[other] {" "}({ $failed } failed)
}: { $name }
import-progress-finished = Upload finished: { $uploaded } uploaded{ $skipped ->
    [0] {""}
   *[other] , { $skipped } already in the library or twice in the folder (skipped)
}{ $failed ->
    [0] {""}
   *[other] , { $failed } failed — select the folder again to retry
}

## New import
import-new = New import
import-pdf-files = { $count } PDF files
import-in-folder = {" "}in “{ $folder }”
import-ignored = { $count ->
    [one] {" "}(1 other file is ignored)
   *[other] {" "}({ $count } other files are ignored)
}
import-structure = Structure
# $preset: name of the structure; $matched of $total files fit it.
import-structure-option = { $preset } — { $matched } of { $total } files
import-own-pattern = Own pattern
import-preset-folder = Folder per piece
import-preset-flat-number = Number and part in the file name
import-preset-flat-title = Title and part in the file name
import-naming-language = Naming language
import-language-de = German: B = B♭, Es = E♭, H = B♮
import-language-de-suggested = German (suggested): B = B♭, Es = E♭, H = B♮
import-language-en = English: B is not read as a key
import-language-en-suggested = English (suggested): B is not read as a key
import-pattern = Pattern
# $placeholders and $ignore are the pattern placeholders; never translate them.
import-pattern-hint = { $placeholders }, { $ignore } = ignore, [ … ] = optional, / = folder
import-summary-pieces = { $count } pieces:{" "}
import-count-ready = { $count } ready
import-count-check = { $count } to check
import-count-complete = { $count } to complete
import-count-unmatched = { $count } files do not fit the pattern
import-unmatched-files = Files that do not fit the pattern
import-size-to-check = { $size } to check. You can review everything before anything is imported.
import-upload-and-review = Upload and review
import-col-number = No.
import-col-parts = Parts
import-title-missing = — title missing —
import-more-pieces = … and { $count } more pieces.

## Review
import-crumb = ‹ Import
import-source-pattern = from the file name
import-source-dictionary = from the instrument dictionary
import-source-alias = from a stored alias
import-source-library = from the library
import-source-librarian = entered by you
import-source-inventory = from the inventory list
import-source-ocr = recognised on the first page
# Other values found for a field.
import-also = {" "}(also: { $values })
import-committed = { $count ->
    [one] { $count } piece imported.
   *[other] { $count } pieces imported.
}
# $error: why a piece was not imported.
import-not-committed = {" "}Not imported: { $error }.
import-undo-confirm = Undo this import? The pieces it created and the parts it added are removed from the library (the files stay on the server). This is refused if musicians already annotated them.
import-undone = Undone: { $pieces } pieces and { $parts } parts removed.
import-count-imported = { $count } imported
import-files-summary = { $total } files: { $uploaded } uploaded, { $duplicate } skipped (already in the library or twice in the folder), { $unmatched } do not fit the pattern
import-resume = { $count } files are not uploaded yet. Select the same folder again to continue; files already uploaded are not sent again.
import-filter = Show
import-filter-all = All pieces
import-filter-attention = Needs attention
import-filter-ready = Ready to import
import-filter-imported = Imported
import-commit-green = Import all green ({ $count })
import-undo = Undo import
import-previous = ‹ Previous
import-next = Next ›
import-page = Page { $page } of { $count }
import-duplicates = { $count } files skipped (duplicates)

## Batch settings
import-language-name-de = German
import-language-name-en = English
import-settings-pattern = Pattern{" "}
import-settings-language = {" "}· naming language { $language }{" "}
import-settings-change = Change
import-presets = Presets:{" "}
import-read-again = Read names again
import-read-again-confirm = Read the file names again? Changes to pieces whose grouping changes are lost; changes to parts are kept.

## Instruments to confirm
import-confirm-title = Instruments to confirm
import-confirm-intro = Musicus never creates instruments by itself: the instrument decides who can read a part. Choose what each word means once; Musicus remembers it for this and every later import.
# $kind: unknown or abbreviation.
import-word-count = {" "}{ $kind ->
    [unknown] unknown
   *[abbreviation] abbreviation
}, in { $count ->
    [one] { $count } file
   *[other] { $count } files
}
import-word-probably = {" "}— probably { $instrument }
import-word-instrument = Instrument for { $word }
import-word-choose = — choose instrument —
import-word-use = Use for “{ $word }”
import-word-create = Create instrument “{ $name }”

## Bulk edit
import-bulk-hint = Tick pieces to give them the same genre, composer or arranger at once.
import-bulk-apply = Apply to { $count } pieces
import-bulk-clear = Clear selection

## Piece and part rows
import-select-piece = Select piece
import-field-number = Number
import-piece-done = ✓ Imported
import-open-piece = Open piece
import-commit-piece = Import piece
import-match = An existing piece matches: { $piece }.{" "}
import-match-add = {" "}Add the parts to it
import-match-new = {" "}Import as a new piece
import-col-page = Page 1
import-col-file = File
import-col-name = Name
import-col-readers = Readable by
import-col-import = Import
import-staves = { $count ->
    [one] , 1 staff per system
   *[other] , { $count } staves per system
}
# Page count, short.
import-pages-short = { $count } p.
import-open-pdf = Open the PDF
import-instruments-of = Instruments of { $path }
import-remove-instrument = Remove instrument { $name }
import-add-instrument = + Add instrument
import-all-instruments-chosen = Every instrument is chosen already.
import-key-of = Key of { $path }
import-readers-none = nobody yet
import-take = Import { $path }
import-voices-of = Voices of { $path }
