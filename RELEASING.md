# Releasing Musicus

How the maintainer makes a release, step by step. Background: chapter "Updates, downgrades and
platforms" of the technical documentation (`docs/technical/chapters/14-updates.typ`): versions, the
downgrade window, database updates and the checks in continuous integration.

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
   `cargo build` so `Cargo.lock` follows. This is the only place where the version is written:
   `musicus-server --version`, `GET /api/version`, the "About Musicus" page and the title pages of the user
   guide and of the technical documentation all take it from there (R-OPS-033…035).
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
6. Run all checks; every command must succeed without warnings:
   ```sh
   cargo test                                                   # also tests the update runner and the merge script (needs python3)
   cargo test -p musicus-web
   cargo clippy --all-targets && cargo fmt --check
   cargo clippy -p musicus-web --target wasm32-unknown-unknown
   ./scripts/fetch-pdfjs.sh && dx build --platform web --package musicus-web
   python3 scripts/check-ui-literals.py
   python3 docs/agents/check.py
   uv run docs/requirements/tools/validate.py
   ```
7. Release checklist:
   - [ ] Patch: no preview, no database update, **no new stored values** (read the diff since the last tag).
   - [ ] 1.0.0 only: `1.0.0.sql` simplified by hand and checked against the appended version (step 5).
   - [ ] Translations complete: `cargo test -p musicus-i18n` passes; new German texts reviewed
         (`scripts/i18n-review.py de`).
   - [ ] User guide (`docs/user-guide`) and technical documentation (`docs/technical`) describe the release;
         screenshots regenerated if the UI changed.
   - [ ] The version is right everywhere (R-OPS-033…035): `cargo run -p musicus-server -- --version` prints
         `musicus-server X.Y.Z`; `docs/user-guide/build.sh` and `docs/technical/build.sh` (no argument: they
         read the workspace version) print "written (version X.Y.Z)" and the title pages say
         "Musicus X.Y.Z". The guide site builds each release with the version of its tagged commit and
         warns if the tag `vX.Y.Z` differs from it (check the workflow log of § 4.4).
   - [ ] Requirement statuses in `docs/requirements` are current; `uv run docs/requirements/tools/validate.py`.
   - [ ] Release notes drafted (§ 5).

## 3. Docs repositories

1. Docs changes go to their own pull requests in `docs/requirements`, `docs/technical` and `docs/user-guide`
   and are merged first.
2. Point the submodules at the merge commits: comment `/update-submodules` on the release pull request
   (the bot commits "Point submodules to merged docs" on the release branch and pushes it; it refuses while a
   docs pull request from a branch of the same name is still open). Without the bot (not set up, see
   [docs/github-setup.md](docs/github-setup.md), or the pull request is not open yet):

   ```sh
   ./scripts/update-submodules.sh && git commit -m "Point submodules to merged docs" && ./scripts/check-submodules.sh && git push
   ```

   If the bot committed, pull its commit (`git pull`) before you continue locally. The tags of the docs
   repositories are created only after the release was published (§ 4, step 5).

## 4. Pull request, merge, publish

Target flow: one release pipeline builds and checks everything first and publishes only when all of it
succeeded. **(planned:** the pipeline does not exist yet; below each step says what to do by hand until
then.)

1. Open the pull request from `release/X.Y.Z` to `main`; all required checks must pass.
   **(planned)** The pipeline runs as a *dry run* on this pull request: it builds and checks everything
   (stage 1 below) and publishes nothing.
   Until then check by hand: patch ⇒ no preview and no `X.Y.Z.sql`; minor ⇒ `X.Y.0.sql` lists exactly the
   deleted previews and `migrations/preview/` is gone; the workspace version is `X.Y.Z` (§ 2 step 2) and
   both title pages say "Musicus X.Y.Z" (§ 2 checklist).
2. Merge the pull request. **(planned)** The merge of a `release/<version>` pull request into `main` starts
   the real run automatically:
   - *Stage 1* (temporary results only, workflow artifacts that expire): workspace version in `Cargo.toml` =
     version in the branch name; all tests, lints, documentation and requirements checks; the database
     release check; program build and smoke test on every platform, with `MUSICUS_RELEASE_BUILD=1` (a release
     build refuses to start with preview updates); linux/amd64 + linux/arm64 images built but not pushed;
     user guide and technical documentation built, their title pages checked to show `X.Y.Z`; the complete
     user guide website built, including the new release.
   - *Stage 2* (only if stage 1 is entirely green), in this order: push the images with their version tags →
     create the tag `vX.Y.Z` on the merge commit → create the GitHub release with the program archives and
     `SHA256SUMS` → deploy the user guide website.
   - If anything fails, nothing is published and the artifacts are discarded: fix the problem (a normal
     pull request) and run the pipeline again. Every publish step skips what already exists, so a run that
     failed while publishing is simply run again.
3. Check the run: every job green; the images `X.Y.Z` and `X.Y` exist; the tag `vX.Y.Z` and the GitHub
   release with the archives and `SHA256SUMS` exist; the user guide site lists `vX.Y.Z` as latest.
4. Until the pipeline exists, publish by hand instead of steps 2 and 3:

   ```sh
   git switch main && git pull && git tag vX.Y.Z && git push origin vX.Y.Z
   ```

   Build the image locally with `MUSICUS_RELEASE_BUILD=1` set for `cargo build --release` (e.g. add it to
   the build command in the `Dockerfile` for that build) and start it once on a copy of an existing data
   directory; nothing is published automatically. Then create the GitHub release for the tag with the
   release notes; publishing it (not as prerelease) runs `.github/workflows/user-guide-pages.yml`, which
   rebuilds the user guide site for all release tags. (In the pipeline this workflow becomes the last step;
   its manual test build stays.)
5. After the release was published successfully, tag the three docs repositories by hand on exactly the
   commits the release points to (always by hand, also with the pipeline):

   ```sh
   git switch main && git pull && git submodule update --init
   for repo in docs/requirements docs/technical docs/user-guide; do
       git -C "$repo" tag vX.Y.Z "$(git rev-parse "vX.Y.Z:$repo")"
       git -C "$repo" push origin vX.Y.Z
   done
   ```

   `git rev-parse vX.Y.Z:<path>` prints the commit the release's submodule pointer names, so the tag lands
   on that commit even if the submodule checkout is elsewhere.

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
