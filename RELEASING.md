# Releasing Musicus

How the maintainer makes a release, step by step. Background: chapter "Updates, downgrades and
platforms" of the technical documentation (`docs/technical/chapters/14-updates.typ`): versions, the
downgrade window, database updates and the checks in continuous integration.

The release pipeline (workflow `Release`, `.github/workflows/release.yml`) builds, checks and publishes;
the maintainer prepares the release pull request and checks the result. Its one-time
GitHub setup is in [docs/github-setup.md](docs/github-setup.md).

## 1. Choose the kind of release

| Kind | Version | May contain |
| --- | --- | --- |
| Patch | `X.Y.Z`, Z > 0 | Fixes only. **No database update** (R-OPS-018) and **no new kind of stored value** — no new role, system-layout region kind, ink format version or error code written into the database (R-OPS-041). An earlier patch of the same minor must read its data. |
| Minor | `X.Y.0` | Features, and at most **one** database update `migrations/X.Y.0.sql`, merged from the previews. |
| Major | `X.0.0` | Like a minor; for large changes. |

How to decide: if any file exists in `crates/musicus-server/migrations/preview/`, or any pull request since
the last release had the label `db-change` (set automatically by CI on every pull request that adds a preview) or
wrote new kinds of stored values, the release **must** be a
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

   Then check that the merge left the database structure unchanged:

   ```sh
   ./scripts/db-schema.sh --check
   ```

   It must print `crates/musicus-server/schema.sql is up to date`. Never regenerate the file here: a difference
   means the merged update creates another structure than the previews did, so the merge is wrong (the CI job
   `db-check` compares the same on the release pull request).
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
     `same schema, rows and sections`; `./scripts/db-schema.sh --check` must still report the file up to date;
     `cargo test` must pass.
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
   - [ ] Patch: no preview, no database update, **no new stored values** (new enum variants, format
         versions) — R-OPS-041. Read the diff since the last tag: no new role, system-layout band kind, layout or
         import state, reader setting, import language, ink format version or kind of mark, error code written
         into the database, placeholder of a stored import pattern or file type.
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
   - [ ] Release notes written in the pull request description, section `## Release notes` (§ 5).

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
   repositories are created by the pipeline in the real run (§ 4, step 5); do not tag them yourself.

## 4. Pull request, merge, publish

The release pipeline builds and checks everything first and publishes only when all of it succeeded.

1. Open the pull request from `release/X.Y.Z` to `main`. Its description must have a section
   `## Release notes` (§ 5): the pipeline takes the notes from there (read when it runs, so later edits
   count) and fails without it. Every push to the branch `release/X.Y.Z` starts the **dry run** (workflow
   `Release`): it builds and checks everything (stage 1 below) and publishes nothing; its checks show on
   the pull request. The first push usually comes before the pull request exists, so that dry run fails
   with "no open pull request": open the pull request, then **Re-run all jobs** of that run (Actions →
   Release, or the failed check on the pull request). The same after editing only the description. The
   dry run and all other required checks must be green. Other pull requests do not start the workflow.
2. **Merge** the pull request (as a person or through the merge queue; use a merge commit, squash or rebase).
   The push of the merge to `main` starts the **real run** automatically:
   - *Stage 1* (temporary results only, workflow artifacts that expire after 7 days):
     - workspace version in `Cargo.toml` and `Cargo.lock` = version in the branch name, newer than on `main`
       before; the database release check (patch: no preview, no update file, nothing under `migrations/`
       changed; minor/major: `X.Y.0.sql` lists exactly the removed previews, none left, `releases.txt`
       gained exactly `X.Y`) and `./scripts/db-schema.sh --check` (`scripts/release-check.py`);
     - submodule pointers, agent-docs check, requirements, UI literals, rustfmt, clippy (host and wasm32),
       client tests;
     - on Linux x86_64 and arm64, Windows x86_64, macOS arm64 and macOS x86_64: `cargo test`, the release
       build with `MUSICUS_RELEASE_BUILD=1` and the web client built in (a release build refuses to start
       with preview updates), a smoke test of the built program (start, web client, version, upload, a
       marking, `backup`, Ctrl+C, second start; `scripts/smoke-program.py`) and its archive. macOS x86_64
       does not block: if it fails, the release notes say that it is not available;
     - the linux/amd64 + linux/arm64 image around these programs, built and started, not pushed;
     - user guide and technical documentation built, their title pages checked to show "Musicus X.Y.Z";
       the complete user guide website built, including the new release;
     - `SHA256SUMS` and the release notes.
   - *Stage 2* (only if stage 1 is entirely green), in this order: push the images `X.Y.Z` and `X.Y` →
     create the tag `vX.Y.Z` on the merge commit and in the three docs repositories (requirements,
     technical documentation, user guide) on the commits the release points to → create the GitHub release `vX.Y.Z` with the program
     archives and `SHA256SUMS` (as a draft first, published once every file is there) → deploy the user
     guide website.
3. **Check** the run (Actions → Release): every job green (a yellow note for macOS x86_64 is allowed); the
   images `X.Y.Z` and `X.Y` exist (by default `ghcr.io/musicus-sheets/musicus`); the tag `vX.Y.Z` exists in
   `Musicus` and in the three docs repositories (the summary of the job `tag` lists them); the GitHub
   release with the archives and `SHA256SUMS` exists; `sha256sum -c SHA256SUMS` succeeds on the downloads;
   the user guide site lists `vX.Y.Z` as latest.
4. **If something fails**, nothing is published until stage 1 is green. Every publish step skips what
   already exists (an image tag `X.Y.Z` is never replaced, an existing tag on the same commit is kept, a
   draft release is completed), so running again finishes what is missing:
   - a flaky job or a failed publish step: **Re-run failed jobs** of the same run;
   - a real problem: fix it in a normal pull request into `main`, then Actions → **Release** → **Run
     workflow**, branch `main`, input `version` = `X.Y.Z` (input `ref` empty = the head of `main`, or the
     commit to release; it must contain the merge of `release/X.Y.Z`). Merge nothing else in between.
     The switch `publish` off gives a dry run of that commit.
   - If the tag `vX.Y.Z` points to another commit, the version is released already: make a new release.
5. The tags of the docs repositories are made by the pipeline too. If a docs repository is tagged on
   another commit already, the job `tag` fails and names it: someone tagged it by hand. Check which
   commit is right (`git rev-parse vX.Y.Z:docs/<name>` in this repository prints the commit the release
   points to), and correct it with the repository's tag ruleset switched off for that moment.

## 5. Release notes

Write them in the release pull request's description under the heading `## Release notes` (everything up
to the next heading of the same level; HTML comments are left out). The pipeline adds the list of
downloads, the image names and, if macOS x86_64 failed, that it is not available. Always mention:

- New features and fixes; for a minor/major that the database is updated at the first start.
- **Removed features** and that their data stays while an older release may still be started.
- **New access restrictions**: after a downgrade the restricted items give no access at all and must be
  granted again by hand after the next upgrade (R-OPS-017).
- **The downgrade window**: an installation can go back to any release of the three previous minor lines
  (in release order), to the release it was upgraded from for three months, and to every patch of the same
  minor (R-OPS-014). Outside it the older release refuses to start and names the oldest one that can.
- Platforms whose non-blocking checks failed (macOS Intel, R-OPS-042): added by the pipeline.

## Building the program

The program is a single file: the web client is built into it (cargo feature `embed-web`). On the system the
program is meant for, with Rust, the `wasm32-unknown-unknown` target and the Dioxus CLI 0.7.10 installed:

```sh
./scripts/fetch-pdfjs.sh                                  # pdf.js for the web client
rm -rf target/dx/musicus-web/release                      # no files of an earlier build
dx bundle --platform web --package musicus-web --release  # web client -> target/dx/musicus-web/release/web/public
MUSICUS_RELEASE_BUILD=1 cargo build --release --locked -p musicus-server --features embed-web
```

The result is `target/release/musicus-server` (`musicus-server.exe` on Windows). The build stops with a
message if the web client was not built first. To build in a client from another folder, set
`MUSICUS_WEB_DIST=<folder>` (relative to the repository root) for the `cargo build`. Check the file: started
with only `--data-dir` on an empty folder, it must print `web client: built into the program` and serve the
login page; started without `--data-dir`, it prints the usual data folder of the system. The Docker image is
built the same way (`Dockerfile`); the release pipeline instead puts the programs it built and tested for
Linux into the image (`Dockerfile` target `release`).

A program built without `--features embed-web` contains no web client and warns at start that only the API
is served; such a build must never be published.

## Troubleshooting

- **The job `checks` of the release pipeline fails** (`release-check: …`): the message names the file. Version ⇒
  § 2 step 2 (also commit `Cargo.lock`); previews or `releases.txt` ⇒ run `scripts/release-migrations.py X.Y.Z` on
  a clean branch; a patch with database changes ⇒ make it a minor. Check locally with
  `scripts/release-check.py --branch release/X.Y.Z origin/main`.
- **The job `plan` fails with "no open pull request"**: open the pull request from `release/X.Y.Z` into
  `main`, then **Re-run all jobs** (or push again).
- **The job `assets` fails with "no section `## Release notes`"**: add the section to the pull request
  description, then **Re-run all jobs**.
- **The job `docs` fails on a title page**: the docs submodules point to commits whose title page does not take
  the version from the build; update the docs and their pointers (§ 3).

- **The CI job `db-check` fails on the release pull request**: on a `release/<version>` branch it only compares
  `crates/musicus-server/schema.sql` with the structure the updates create. The merged update (or its
  simplification by hand) differs from the previews: fix the update, not the snapshot (§ 2, step 3).

- **The merge script refuses** (`release-migrations: …`, exit 1, nothing written): a patch with previews ⇒
  make it a minor; "already released" ⇒ wrong version or the line is in `releases.txt` already; a preview
  named wrongly or with a broken section marker ⇒ fix it in a normal pull request first (never edit a preview
  that is already on `main`: add a new one).
- **A development database is "in an unclear state"** at start: it applied only some of the previews that a
  release update now contains (typically a demo database from a branch). Move its data directory away and
  start with a fresh demo database.
- **"This data was last used by Musicus …; start Musicus … or newer."**: the release is outside the
  downgrade window of that data; start the release named in the message.
