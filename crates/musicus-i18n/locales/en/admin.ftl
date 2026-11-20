# Instruments, groups and users pages.

## Instruments
admin-instruments-title = Instruments
admin-instruments-intro = The instruments of the ensemble. Every part is tagged with the instruments it is for, and groups get access to the parts by instrument.
admin-instruments-add-title = Add an instrument
admin-instruments-name = Instrument
admin-instruments-name-placeholder = Instrument, e.g. Trumpet
admin-instruments-family = Family
admin-instruments-family-placeholder = Family, e.g. Brass
# Heading of instruments without family (listed last).
admin-instruments-other-family = Other
admin-instruments-add = + Add
# $name: the instrument just added.
admin-instruments-added = Added “{ $name }”.
admin-instruments-none = No instruments yet.
admin-instruments-none-manage = No instruments yet. Add the instruments of your ensemble above.
admin-instruments-delete-confirm = Delete the instrument “{ $name }”?

## Groups
admin-groups-title = Groups
admin-groups-intro = Groups bundle musicians who read the same music. Members of a group can read every part tagged with a granted instrument, plus every part of a granted score.
admin-groups-intro-not-admin = {" "}Only admins can create groups and change their members.
admin-groups-member-count = { $count ->
    [one] { $count } member
   *[other] { $count } members
}
admin-groups-new-placeholder = New group, e.g. Trumpets
admin-groups-add = Add
admin-groups-select = Select a group to edit its members and access.
admin-groups-delete-confirm = Delete this group? Its members lose the access it granted.
admin-groups-editor-intro = Members can read every part tagged with a granted instrument, plus every part of a granted score. Changes are saved right away.
admin-groups-members = Members
admin-groups-add-members = + Add members
admin-groups-no-members = No members yet.
admin-groups-search-members = Search name or username…
admin-groups-instruments = Instruments
admin-groups-add-instruments = + Add instruments
admin-groups-no-instruments = No instruments granted yet.
admin-groups-scores = Whole scores (e.g. for the conductor)
admin-groups-add-scores = + Add scores
admin-groups-no-scores = No whole scores granted.
admin-groups-delete = Delete group

## Users
admin-users-title = Users
# $name (in bold): the new user.
admin-users-invite-text = Send this one-time link to { $name }. It is valid for 7 days and lets them choose a password.
admin-users-copy = Copy
admin-users-new = New user
admin-users-display-name = Display name
admin-users-create = Create and get invite link
admin-users-col-name = Name
admin-users-col-role = Role
admin-users-col-status = Status
admin-users-disabled = Disabled
admin-users-active = Active
admin-users-invited = Invited
admin-users-reset-link = Reset link
admin-users-new-invite = New invite link
admin-users-enable = Enable
admin-users-disable = Disable
