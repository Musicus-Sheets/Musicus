# Shared texts of the web client (a-i18n-keys: <area>-<page or component>-<what>).

# Shown while a page loads.
common-loading = Loading…

# File sizes; $size is already formatted for the language (decimal separator).
common-size-mb = { $size } MB
common-size-kb = { $size } KB

# Page count of a PDF.
common-pages = { $count ->
    [one] { $count } page
   *[other] { $count } pages
}

## Navigation bar
common-nav-menu = Menu
common-nav-library = Library
common-nav-programmes = Programmes
common-nav-instruments = Instruments
common-nav-groups = Groups
common-nav-import = Import
common-nav-users = Users
common-nav-log-out = Log out

## Pages that do not exist or are not allowed
common-no-access-title = Not available
common-no-access-text = Your role does not include this page.
common-back-to-library = Back to the library
common-not-found-title = Page not found
# $path: the address typed.
common-not-found-text = { $path } does not exist.

## Buttons and words used everywhere
common-close = Close

## Assignment picker (chips + dropdown)
# $name: the entry (a person, instrument or piece).
common-remove-entry = Remove { $name }
common-picker-none-left = Everything is assigned already.
common-picker-no-matches = No matches.
common-picker-tick = Tick one or more
common-picker-selected = { $count } selected
# Button that adds the ticked entries.
common-picker-add = Add { $count }

## Chip entry of tags and voices
# $name: the tag.
common-tag-remove = Remove tag { $name }
# $name: the voice ("2").
common-voice-remove = Remove voice { $name }
common-edit = Edit
common-save = Save
common-cancel = Cancel
common-delete = Delete
common-saving = Saving…
common-saved = Saved

## Roles
common-role-member = Member
common-role-librarian = Librarian
common-role-admin = Admin
# A role saved by a newer version of Musicus; such a user cannot log in with this version, and the admin page offers no role change for them.
common-role-unknown = Unknown role (newer version, cannot log in)

## Search fields of pickers
common-search-instruments = Search instrument or family…
common-search-pieces = Search title or composer…

## Connection to the server (banner at the bottom of every page, a-client-outage)
# The server announced its stop (an update or a restart).
common-connection-shutting-down = Server is shutting down. You can keep reading; drawing and changes are paused until it is back.
# The server announced that it is starting (database updates may take a while).
common-connection-starting = Server is starting. You can keep reading; drawing and changes are paused until it is ready.
# No answer without an announcement (network loss).
common-connection-unreachable = Server not reachable. You can keep reading and drawing; your annotations are saved when the connection is back.
# The server runs a newer version than the open page.
common-connection-updated = Musicus has been updated. The page reloads by itself when you are not reading or editing.
common-connection-reload = Reload now
