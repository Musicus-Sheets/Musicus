# GitHub setup: bot for `/update-submodules`

A comment `/update-submodules` on a pull request of this repository runs the workflow
`Update submodules` (`.github/workflows/update-submodules.yml`). It moves the pull request's
submodule pointers (`docs/requirements`, `docs/technical`, `docs/user-guide`) to the current
`main` of each docs repository, commits "Point submodules to merged docs" and pushes to the pull
request's branch. It does nothing, and says why in a reply, when:

- a docs repository still has an open pull request from a branch with the same name as the
  pull request's branch (merge it first, then comment again);
- the pull request comes from a fork, is closed, or its branch is `main`;
- the person commenting has no write access to this repository.

The workflow pushes with a GitHub App, because commits pushed with the workflow's own
`GITHUB_TOKEN` start no workflows, so the pull request's checks would not run on the new commit.
The app is used for nothing else (checkout and push of the pull request's branch). It needs
the one-time setup below; until it is done, the comment fails and the manual
commands in [RELEASING.md](../RELEASING.md) § 3 still work.

## One-time setup

1. Create the app: organisation **Musicus-Sheets** → **Settings** → **Developer settings** →
   **GitHub Apps** → **New GitHub App**.
   - Name: for example `musicus-bot` (commits appear as `<name>[bot]`); homepage URL: the repository.
   - **Webhook**: untick **Active** (the app receives no events; the workflow only uses its token).
   - **Repository permissions**: Contents: *Read and write*; Metadata: *Read-only*
     (preselected). Everything else *No access*, also Pull requests and Issues: the app only
     pushes the commit. Reading the pull request, the reactions and the replies use the workflow's
     own token, so they appear as `github-actions[bot]`.
   - **Where can this GitHub App be installed?**: *Only on this account*.
2. On the app's settings page (**Edit** next to the app): note the **Client ID** (starts with `Iv`)
   under **About**. Then, in the left sidebar, **Credentials** → **Key pairs** → **New key**
   (a `.pem` file is downloaded). Keys do not expire; to replace a key, create the new one first,
   then delete the old one.
3. **Install App** → Musicus-Sheets → **Only select repositories** → `Musicus` (not the docs
   repositories; they are public and only read).
4. In the repository `Musicus` → **Settings** → **Secrets and variables** → **Actions**:
   - tab **Variables**: new repository variable `MUSICUS_BOT_CLIENT_ID` = the Client ID;
   - tab **Secrets**: new repository secret `MUSICUS_BOT_PRIVATE_KEY` = the whole content of the
     `.pem` file. Delete the downloaded file afterwards.
5. If a branch ruleset or branch protection restricts who may push to pull request branches, add
   the app to its bypass list (or allow it to push). Rules that require signed commits reject the
   bot's commits (they are not signed).

## Try it

The workflow only runs in the version on `main` (GitHub runs comment-triggered workflows from the
default branch), so it works once it has been merged.

- **Actions** → **Update submodules** → **Run workflow**, branch `main`, input `pr` = the number of
  a pull request. The run replies on that pull request like a comment would.
- Or comment `/update-submodules` (exactly that, nothing else) on a pull request. The bot reacts
  with 👀 when it starts, then 🚀 (pushed), 👍 (already up to date), 😕 (refused, with the reason)
  or 👎 (failed, with the error and a link to the run).
