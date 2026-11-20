# Library, piece and part editors.

## Library list
library-title = Library
library-search = Search title, composer, tag, number…
library-sort = Sort by
library-sort-title = Sort by title
library-sort-number = Sort by archive number
library-import-folder = Import folder
library-import-folder-hint = Import a whole folder of PDF parts
# Button; a "+" is shown in front of it.
library-add-piece = Add piece
library-nothing-found = Nothing found.
library-empty-manage = The library is empty. Add the first piece with “+ Add piece”.
library-empty = No sheets have been shared with you yet.
library-archive-number = Archive number
# Badge before the title; $number as entered.
library-archive-number-badge = No. { $number }
library-no-parts = no parts yet
library-parts-without-pdf = { $count ->
    [one] { $count } part without PDF
   *[other] { $count } parts without PDF
}
library-edit-piece = Edit this piece
library-edit-part = Edit this part
library-no-file = No file uploaded yet

## Guided flow "Add piece" and piece editor
library-crumb = ‹ Library
library-steps = Progress
library-step-details = Details
library-step-parts = Parts and PDFs
library-details-hint = Only the title is required. Genre and tags help musicians find the piece.
library-next-parts = Next: parts ›
# $title: the piece.
library-parts-of = Parts of “{ $title }”
library-parts-hint = Add one part per sheet: its instruments, key and voices give its name, e.g. “Trumpet in B♭ 1”; a custom name is optional. Groups get access by instrument.
library-parts-none = No parts yet.
library-add-first-part = Add the first part
library-add-another-part = Add another part
library-back-details = ‹ Details
library-waiting-uploads = Waiting for uploads…
library-finish = Finish
library-edit-piece-title = Edit piece
library-done = Done
library-save-details = Save details
library-parts-count = Parts ({ $count })
library-add-part-title = Add a part
library-delete-piece = Delete piece
library-delete-piece-confirm = Delete this piece with all its parts? Everyone's annotations on it are deleted too.
# $warning (in bold): library-delete-piece-warning.
library-delete-piece-text = Deletes the piece with all its parts and files from the library. { $warning } Programmes keep their other pieces.
library-delete-piece-warning = Everyone's annotations on it are deleted too.
library-edit-part-title = Edit part
# Followed by a link "Edit piece".
library-edit-part-more = Other parts, the details of the piece and deleting it:{" "}

## Piece fields
library-field-title = Title
library-title-placeholder = e.g. Florentiner Marsch
library-field-subtitle = Subtitle
library-field-composer = Composer
library-field-arranger = Arranger
library-field-genre = Genre
library-genre-placeholder = March, polka, concert…
library-field-tags = Tags
library-tags-placeholder = e.g. Christmas, Encore
library-tags-hint = Press comma or Enter after each tag.
library-field-notes = Notes

## Part fields
# Followed by a link to the Instruments page.
library-no-instruments = No instruments yet.{" "}
library-no-instruments-link = Add them on the Instruments page.
library-part-no-instruments = No instruments yet: nobody gets this part through an instrument grant.
library-field-key = Key
library-key-none = — (concert pitch)
# A stored key that is not in the list (older data), as stored.
library-key-other = Other: { $key }
library-field-voices = Voices
library-voices-placeholder = e.g. 1, 2 or Solo
library-field-custom-name = Custom name (optional)
library-custom-name-placeholder = e.g. Solo Cornet
library-shown-as = Shown as:{" "}
library-instruments-saved = Instruments (saved right away)

## Files of a part
# $error: the browser's message.
library-file-unreadable = Could not read the file: { $error }
# $size: formatted file size.
library-file-version = Version { $version } · { $size }
# $file: version, size, pages; $time: date and time.
library-file-uploaded = { $file } · uploaded { $time }
library-open = Open
library-edit-systems = Edit systems
library-no-pdf = No PDF yet
library-uploading = Uploading…
library-replace-pdf = Replace PDF…
library-upload-pdf = Upload PDF…
library-uploading-file = Uploading { $name }…
library-uploaded-version = Uploaded as version { $version }
library-all-versions = All versions ({ $count })
library-version = Version { $version }
library-version-current = current
library-versions-hint = Old versions stay stored, so markings made on them are kept and can be moved to the current version.
library-save-part = Save part
library-delete-part = Delete part
library-delete-part-confirm = Delete this part? Everyone's annotations on it are deleted too.
library-choose-pdf = Choose PDF…
library-choose-another-pdf = Choose another PDF…
library-no-file-chosen = No file chosen. You can also upload it later.
library-add-part = + Add part
library-upload-failed = Upload failed: { $error }
