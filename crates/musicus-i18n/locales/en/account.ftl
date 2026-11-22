# Login, invite and account pages.

## Login
login-username = Username
login-password = Password
login-submit = Log in
login-no-account = No account yet? Ask your librarian for an invite link.
login-wrong-password = Wrong username or password.

## Invite (setting the first password)
login-invite-mismatch = The passwords do not match.
login-invite-expired-title = Link expired
login-invite-expired-text = This invite link is invalid or has expired. Ask your librarian for a new one.
login-invite-go-to-login = Go to login
# $name: the user's display name.
login-invite-welcome = Welcome, { $name }
# $username is shown in bold.
login-invite-choose = Choose a password for the username { $username }.
login-invite-repeat = Repeat password
login-invite-submit = Set password and log in

## Account
account-title = Account
# $username is shown in bold; $role is member, librarian or admin.
account-user-line = { $name } · username { $username } · role { $role ->
    [librarian] librarian
    [admin] admin
   *[member] member
}
account-change-password = Change password
account-current-password = Current password
account-new-password = New password (at least 8 characters)
account-password-changed = Password changed. Your other devices were logged out.
account-devices = Logged-in devices
account-unknown-device = Unknown device
# $time: date and time of the last request.
account-last-used = Last used { $time }
account-this-device = {" · "}this device
account-log-out-device = Log out

## About (the "About Musicus" page, reachable with and without a login)
# Link on the login page and at the end of the account page.
about-link = About Musicus
about-title = About Musicus
# Label of the release version, e.g. "0.1.0".
about-version-label = Version
about-licence-label = Licence
# Name of the licence; links to its text. The short name "AGPL-3.0-or-later" follows in brackets.
about-licence-name = GNU Affero General Public License, version 3 or later
about-source-label = Source code
about-free-software = Musicus is free software: you may use, study, share and change it under the terms of this licence.
about-go-to-login = Go to login
