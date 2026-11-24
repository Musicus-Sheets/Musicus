# The reader (full-screen part view) and the system editor.

reader-no-file = No file has been uploaded for this part yet.
reader-open-failed = Could not open the sheet: { $error }
reader-annotations-load-failed = Could not load annotations: { $error }
# Shown when drawing on a page whose saved annotations this version cannot read (e.g. saved by a newer version).
reader-annotations-page-locked = The annotations of this page cannot be shown or changed with this version of Musicus. They stay saved as they are; nothing drawn here would be kept.
reader-set-create-failed = Could not create an annotation set: { $error }
reader-settings-failed = Could not save the view setting: { $error }
reader-layout-failed = The system view is not available for this sheet; showing whole pages.
reader-layout-pending = The systems of this sheet are still being detected; showing whole pages for now.
reader-save-pending = Unsaved
reader-save-failed = Not saved: { $error }
# Annotations drawn while the server is away or could not be saved yet (a-client-outage).
reader-annotations-kept = Annotations not saved yet are kept in this tab and saved when the server is back. Keep this tab open.
# The server refused to store annotations (e.g. it was updated meanwhile); $error: its reason.
reader-annotations-refused = These annotations couldn't be saved: { $error }. They are kept in this tab until you close it.
# In the tool bar while the server announced that it is away.
reader-draw-paused = Drawing paused
reader-dismiss = Dismiss
reader-next-piece = Next piece ›

## Bar
reader-back = Back
reader-system-view = ☰ Systems
reader-system-view-hint = System view: whole systems at a minimum staff size, across pages
reader-staff-size = Staff size
reader-draw = ✎ Draw
reader-draw-done = ✓ Done
reader-draw-hint = Draw (annotations are private)
reader-hide-annotations = Hide annotations
reader-show-annotations = Show annotations
reader-full-screen = Full screen

## Tools
reader-tool-pen = Pen
reader-tool-highlighter = Highlighter
reader-tool-eraser = Eraser
reader-tool-text = Text
reader-tool-stamp = Stamp
reader-tool-stamp-choose = Stamp…
reader-text-prompt = Text
reader-undo = Undo
reader-redo = Redo
reader-color-red = Red
reader-color-blue = Blue
reader-color-black = Black
reader-color-green = Green
reader-color-yellow = Yellow
reader-color-pink = Pink
reader-width-fine = Fine
reader-width-medium = Medium
reader-width-bold = Bold

## Annotation sets menu
reader-sets-title = Annotation sets
reader-sets-none = None yet. Drawing creates one.
reader-sets-default-tag = default
# Tag of a set drawn on an older file version.
reader-sets-version-tag = v{ $version }
reader-sets-no-annotations = No annotations
reader-sets-new = New set
reader-sets-duplicate = Duplicate
reader-sets-rename = Rename
reader-sets-rename-prompt = New name
reader-sets-make-default = Make default
reader-sets-use-for-programme = Use for this programme
reader-sets-name-prompt = Name of the new annotation set
# Suggested names in the prompt (the user may change them; stored as entered).
reader-sets-suggest-new = Concert
reader-sets-suggest-copy = Copy
reader-sets-delete-confirm = Delete this annotation set with all its drawings?
reader-sets-outdated = These annotations belong to version { $version } of the sheet; the current version is { $current }.{" "}
reader-sets-migrate = Move them to the current version

## System editor (librarians)
reader-systems-crumb = ‹ Edit part
reader-systems-title = Systems
# $part: the part's name.
reader-systems-subtitle = { $part } · current version
reader-systems-no-pdf = This part has no PDF yet.
reader-systems-render-failed = Could not render the page: { $error }
reader-systems-saved = Saved — every member now sees this page this way
reader-systems-reset-done = Back to the automatic detection
reader-systems-discard-confirm = Discard the changes on this page?
reader-systems-pending = The systems of this file are still being detected. Try again in a few seconds.
reader-systems-failed = The systems of this file could not be detected; the reader shows its pages whole.
reader-systems-reload = Reload
reader-systems-hint = Each coloured band is one line of music (system) as the system view shows it. Drag a red line to move a cut, “Split” a band that holds two systems, × removes a cut (joins two bands). Headers are shown with the first page, hidden bands never. Saved changes apply to every musician.
reader-systems-previous-page = ‹ Previous page
reader-systems-next-page = Next page ›
reader-systems-page = Page { $page } / { $count }
reader-systems-corrected = corrected
reader-systems-confidence = confidence { $value }
reader-systems-whole-page = Show this page as a whole page
reader-systems-kind-system = System
reader-systems-kind-header = Header (title)
reader-systems-kind-footer = Footer
reader-systems-kind-hidden = Hidden
reader-systems-band-kind = Kind of band { $band }
reader-systems-split-band = Split band { $band }
reader-systems-split = Split
reader-systems-split-hint = Split this band in two (then drag the new line)
reader-systems-cut = Cut { $cut }
reader-systems-remove-cut = Remove cut { $cut }
reader-systems-remove-cut-hint = Remove this cut: join the bands above and below
reader-systems-save = Save page
reader-systems-reset = Reset to automatic
reader-systems-unsaved = Unsaved changes
# Shown in the systems editor for a page whose saved layout or correction this version cannot read.
reader-systems-newer-version = This page was saved by a newer version of Musicus. It is shown whole and cannot be edited with this version; a correction can only be reset.
