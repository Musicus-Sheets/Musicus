# Releasing Musicus

How the maintainer makes a release, step by step. Background: `docs/technical/chapters/14-updates.typ`
(updates, downgrade window, CI checks) and the decisions `d-update-model`, `a-preview-migrations`,
`a-update-runner`, `a-patch-no-db-changes`, `a-db-check` in `docs/agents/decisions.md`.

Steps marked **(planned)** are not automated yet; do what the step says under "until then".

## 1. Choose the kind of release

| Kind | Version | May contain |
| --- | --- | --- |
| Patch | `X.Y.Z`, Z > 0 | Fixes only. **No database update** (R-OPS-018) and **no new kind of stored value** — no new role, system-layout region kind, ink format version or error code written into the database (R-OPS-041). An earlier patch of the same minor must read its data. |
| Minor | `X.Y.0` | Features, and at most **one** database update `migrations/X.Y.0.sql`, merged from the previews. |
| Major | `X.0.0` | Like a minor; for large changes. |

How to decide: if any file exists in `crates/musicus-server/migrations/preview/`, or any pull request since
the last release had the label `db-change` or wrote new kinds of stored values, the release **must** be a
minor (or major). Otherwise a patch is possible. Old minors get no patches: a fix always comes with the next
release.

## 2. Prepare the release branch

1. Branch from `main`: `git switch -c release/X.Y.Z`. Only a branch named `release/<version>` may delete
   previews and add the merged database update.
2. Raise the version in the workspace `Cargo.toml` (`[workspace.package] version = "X.Y.Z"`), then
   `cargo build` so `Cargo.lock` follows.
3. Merge the database updates (minor/major; for a patch it only checks that there are none):

   ```sh
   scripts/release-migrations.py X.Y.Z
   ```

   It merges every `migrations/preview/*.sql` (in name order) into `migrations/X.Y.0.sql`, with the header
   line `-- @previews: …` listing them, deletes the preview files and appends the line `X.Y` to
   `migrations/releases.txt` (the release order the downgrade window is counted on). A minor without previews
   only gets its line in `releases.txt` (no update file). It changes nothing and exits with a message when the
   release is a patch but previews exist, when the line is already released, or when the version is older
   than the last release.

   The previews are first appended in order; that version is the reference. Then the script tries safe
   simplifications (an `ADD COLUMN` or `RENAME COLUMN` folded into a `CREATE TABLE` of the same cycle; a
   table, index, trigger, view or column created and dropped again disappears) where no data step lies in
   between, and keeps them only if the simplified update gives the same schema and rows as the appended one
   (empty database plus the earlier release updates). Pass `--fixture <db>` with a database of the previous
   release holding test data to compare its rows too (fixtures per release are planned for phase 2). The
   output lists each simplification and the verification result; if they diverge it says where and writes
   the appended version. `--no-simplify` skips the attempt.
4. Review the merged update section by section:
   - `-- @additive`: only new tables, nullable/defaulted columns, indexes and copies — nothing dropped.
   - `-- @rebuild`: refills the new structures from the old ones (runs after an older release used the data).
   - `-- @removal`: drops and destructive rewrites; runs later, by itself, once no release in the window needs
     the old structures. Restrictions of access must take the restricted item away from older releases
     (R-OPS-017).
5. **Only for 1.0.0:** the merged `migrations/1.0.0.sql` contains the whole development history (the
   development migrations 0001–0010 and later previews), only partly simplified by the script. Simplify it by
   hand to the final schema:
   - start from the schema of a fresh database with all previews (`sqlite3 data/musicus.sqlite .schema`
     after starting the program before the merge on an empty data directory), keep the tables' column order,
     the comments worth keeping, `-- @previews:` (unchanged) and the section markers;
   - check it against the appended version: keep the script's output as `appended.sql` (or run
     `scripts/release-migrations.py --no-simplify 1.0.0` on a copy of the tree), then
     `scripts/release-migrations.py --compare appended.sql crates/musicus-server/migrations/1.0.0.sql`
     (add `--fixture <db>` when a database with test data exists) must print
     `same schema, rows and sections`; `cargo test` must pass.
   From 1.1 on this step is not needed: the automatic simplification is enough, the update stays as written.
6. Run every check of `AGENTS.md` § Commands, plus `cargo test` (it also tests the merge script and the
   update runner).
7. Release checklist:
   - [ ] Patch: no preview, no database update, **no new stored values** (read the diff since the last tag).
   - [ ] 1.0.0 only: `1.0.0.sql` simplified by hand and checked against the appended version (step 5).
   - [ ] Translations complete: `cargo test -p musicus-i18n` passes; new German texts reviewed
         (`scripts/i18n-review.py de`).
   - [ ] User guide (`docs/user-guide`) and technical documentation (`docs/technical`) describe the release;
         screenshots regenerated if the UI changed.
   - [ ] The title pages show the version: `version:` in `docs/user-guide/main.typ` and
         `docs/technical/main.typ` (R-OPS-035). **(planned:** taken from the release version
         automatically; until then edit both by hand.)
   - [ ] Requirement statuses in `docs/requirements` are current; `uv run docs/requirements/tools/validate.py`.
   - [ ] Release notes drafted (§ 5).

## 3. Docs repositories

1. Docs changes go to their own pull requests in `docs/requirements`, `docs/technical` and `docs/user-guide`
   and are merged first.
2. Point the submodules at the merge commits: `./scripts/update-submodules.sh`, commit on the release branch.
3. Tag the requirements repository on exactly the pinned commit (R-OPS-036):

   ```sh
   git -C docs/requirements tag vX.Y.Z && git -C docs/requirements push origin vX.Y.Z
   ```

## 4. Pull request, merge, tag, publish

1. Open the pull request from `release/X.Y.Z` to `main`; all required checks must pass.
   **(planned:** release check on `release/<version>` branches and the database check, R-OPS-038…040 —
   until then check by hand: patch ⇒ no preview and no `X.Y.Z.sql`; minor ⇒ `X.Y.0.sql` lists exactly the
   deleted previews and `migrations/preview/` is gone.)
2. Merge, then tag the merge commit and push the tag:

   ```sh
   git switch main && git pull && git tag vX.Y.Z && git push origin vX.Y.Z
   ```

3. **(planned)** Release workflow on the tag: builds with `MUSICUS_RELEASE_BUILD=1` (a release build
   refuses to start with preview updates, R-OPS-024), publishes the linux/amd64 + linux/arm64 image with
   version tags (R-OPS-028) and program archives per platform with `SHA256SUMS` (R-OPS-029).
   Until then: build the image locally with `MUSICUS_RELEASE_BUILD=1` set for `cargo build --release`
   (e.g. add it to the build command in the `Dockerfile` for that build) and start it once on a copy of an
   existing data directory; nothing is published automatically.
4. Create the GitHub release for the tag with the release notes. Publishing it (not as prerelease) runs
   `.github/workflows/user-guide-pages.yml`, which rebuilds the user guide site for all release tags.

## 5. Release notes

Always mention:

- New features and fixes; for a minor/major that the database is updated at the first start.
- **Removed features** and that their data stays while an older release may still be started.
- **New access restrictions**: after a downgrade the restricted items give no access at all and must be
  granted again by hand after the next upgrade (R-OPS-017).
- **The downgrade window**: an installation can go back to any release of the three previous minor lines
  (in release order), to the release it was upgraded from for three months, and to every patch of the same
  minor (R-OPS-014). Outside it the older release refuses to start and names the oldest one that can.
- Platforms whose non-blocking checks failed (macOS Intel, R-OPS-042), once those checks exist.

## Troubleshooting

- **The merge script refuses** (`release-migrations: …`, exit 1, nothing written): a patch with previews ⇒
  make it a minor; "already released" ⇒ wrong version or the line is in `releases.txt` already; a preview
  named wrongly or with a broken section marker ⇒ fix it in a normal pull request first (never edit a preview
  that is already on `main`: add a new one).
- **A development database is "in an unclear state"** at start: it applied only some of the previews that a
  release update now contains (typically a demo database from a branch). Move its data directory away and
  start with a fresh demo database.
- **"This data was last used by Musicus …; start Musicus … or newer."**: the release is outside the
  downgrade window of that data; start the release named in the message.
