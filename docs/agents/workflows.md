# Workflows, commands, pitfalls

## Build & test

```sh
cargo build                       # api + server (web excluded from default-members)
cargo test                        # api unit, i18n unit + catalogue checks, server unit, API integration tests, no_prose, release scripts + program smoke test (tests/release_tools.rs; needs python3, git)
cargo test -p musicus-web         # client unit tests on the host (errors.rs: texts, pseudo-locale check); not in default-members
cargo run --release -p musicus-server --example layout_report -- a.pdf …   # system detection per page (regressions)
cargo clippy --all-targets        # must be warning-free
cargo fmt --check                 # rustfmt.toml: max_width 120
cargo clippy -p musicus-web --target wasm32-unknown-unknown
# CI (workflow `Rust`, .github/workflows/rust.yml) runs fmt, clippy, clippy-wasm, test and test-web as
# parallel jobs with RUSTFLAGS=-D warnings; when the Docker build's Rust version changes, change RUST_TOOLCHAIN too.
./scripts/fetch-pdfjs.sh          # once (again after changes of the script); populates crates/musicus-web/public/vendor/pdfjs (gitignored)
dx build --platform web --package musicus-web            # debug → target/dx/musicus-web/debug/web/public
dx bundle --platform web --package musicus-web --release # release (needs wasm-opt)
cargo run -p musicus-server -- serve --data-dir /tmp/m --web-dir target/dx/musicus-web/debug/web/public --insecure-cookies
# single file (a-embedded-web): client from `dx bundle --release` built into the program; clean the folder first so no stale files get in
rm -rf target/dx/musicus-web/release && dx bundle --platform web --package musicus-web --release && cargo build --release -p musicus-server --features embed-web
MUSICUS_WEB_DIST=crates/musicus-server/tests/fixtures/web cargo test -p musicus-server --features embed-web --test embedded_web   # CI does this; clippy too
node e2e/smoke.mjs <admin-invite-url-from-log> some.pdf /tmp/shots   # fresh data dir required; BROWSER=firefox for Firefox
python3 scripts/check-ui-literals.py  # no literal UI text in the client (R-I18N-010; CI: workflow `UI literals`)
./scripts/db-schema.sh [--check]      # regenerate (or compare) crates/musicus-server/schema.sql after a preview change (a-db-check)
python3 scripts/check-db-changes.py --branch "$(git branch --show-current)" origin/main   # CI database check, git part (R-OPS-038)
node e2e/pseudo.mjs <admin-invite-url>   # pseudo-locale run (fresh data dir; every UI text marked)
node e2e/german.mjs <admin-invite-url> [shots]   # German run (fresh data dir; de-DE browser; screenshots + phone-width overflow with a dir)
node e2e/outage.mjs target/debug/musicus-server <web dir> 8191 [shots]   # stop/start/update with an open reader; starts its own server (port and port+1)
python3 scripts/i18n-review.py de > review-de.md   # side-by-side English | German for the maintainer's review (not shipped)
python3 docs/agents/check.py [--since <ref>]
uv run docs/requirements/tools/validate.py
docker compose up -d --build
```

Toolchain: Rust ≥ 1.88 (edition 2024, let-chains used), target `wasm32-unknown-unknown`, `dx` 0.7.10 (must match dioxus crate version), wasm-bindgen CLI version must equal the `wasm-bindgen` crate version in Cargo.lock (dx downloads it).

## Environment pitfalls (seen in practice)

- No local wasm toolchain? Build an image `FROM rust:1.97-bookworm` + `rustup target add wasm32-unknown-unknown` +
  `cargo install dioxus-cli --version 0.7.10`, mount the repo, set `CARGO_TARGET_DIR=/src/target/docker`, and run the
  web clippy / `dx build` inside it; serve the result with a host-built server (`--web-dir target/docker/dx/...`).
  For the single file: `MUSICUS_WEB_DIST=target/docker/dx/musicus-web/release/web/public cargo build --release -p musicus-server --features embed-web`.
- Without `--data-dir` the server uses the OS folder (`~/.local/share/musicus`), or `./data` if that holds a database
  (a-data-dir-default): always pass `--data-dir` in tests and e2e runs.
- Sharpness tests: Playwright's `deviceScaleFactor` (CDP DSF emulation) lays out in CSS pixels and scales the result, so
  fractional CSS sizes always look resampled there; use a real factor (`chromium.launch({args:
  ['--force-device-scale-factor=2']})` + `viewport: null`) and compare `devicePixelContentBoxSize` with the canvas size
  (a-device-pixels). Screenshot comparisons: crop the full device-pixel screenshot, a fractional `clip` resamples.
- Playwright: `waitForLoadState('networkidle')` never resolves while the event stream `/api/events` is open (every logged-in page); wait for the element you need or use an `idle()` helper that ignores that URL (`e2e/german.mjs`).
- Playwright: the npm package version must match the installed browser revisions (`~/.cache/ms-playwright`).
- Changing detection results: raise `layout::ALGORITHM_VERSION`; regression-check with `examples/layout_report.rs`
  on local scores (never commit copyrighted scores or images derived from them; `/example_scores/` is ignored).

- `dx` downloads esbuild, wasm-bindgen, wasm-opt; behind TLS-intercepting proxies the download fails ("UnknownIssuer"). Fix: put the tools on PATH and set `NO_DOWNLOADS=1` (dx then uses PATH). Docker: build arg `RUST_IMAGE=<image with tools + NO_DOWNLOADS=1>`.
- `dx` tool cache: `~/.local/share/.dx/tools/<tool>-<version>/`.
- Old `wasm-opt` (< binaryen 116+ features) fails to parse the module; dx continues unoptimised (larger wasm).
- Modern pdf.js build fails on browsers without `Map.prototype.getOrInsertComputed` → keep legacy build.
- Docker Hub may rate-limit anonymous pulls (429); `mirror.gcr.io/library/<image>` works.
- Session cookies are `Secure`: over plain HTTP on a non-localhost host logins do not stick; use `--insecure-cookies` for such tests only.
- Integration tests set `cookie_secure: false` and use bearer tokens mostly.
- Typst (human docs): `context` is a keyword (cannot be a parameter name); `<word>` in markup is a label; `@x` is a reference.

## Recipes

### Add an endpoint
1. Types in `crates/musicus-api/src/dto.rs`.
2. Handler in `crates/musicus-server/src/routes/<area>.rs`: `AuthUser` param; role check; `part_visible!()` for anything part-related (INV-1); validate with `error::required/optional`; static SQL (INV-5); writes on `app.db.write` in a tx if >1 statement.
3. Route in `routes/mod.rs::api()`.
4. Integration test in `tests/api.rs` (incl. denied ⇒ 404 / 403 cases).
5. Client call via `crates/musicus-web/src/api.rs` helpers.
6. Docs: `server.md` endpoint table, `api-and-ink.md` if new conventions, `code-map.md`, requirement file(s) (`implemented_by`, `verified_by`, status).

### Change documentation in a submodule (docs/requirements, docs/technical, docs/user-guide)
1. In the submodule: `git switch -c <branch>`, commit, push, open a pull request in that repository.
2. In the main repo: commit the new pointer on your branch (CI check `pinned` stays red for now; that is intended).
3. After the docs pull requests are merged into their `main`: comment `/update-submodules` on the main pull request
   (workflow `Update submodules`, a-update-submodules-bot: refuses while a docs PR from a branch of the same name is
   open, for forks and for commenters without write access; otherwise runs the default branch's
   `scripts/update-submodules.sh` on the PR branch, commits "Point submodules to merged docs" as the app's bot,
   runs `scripts/check-submodules.sh`, pushes, replies with old → new pointers). Pull the bot's commit before
   working on. Fallback by hand: `./scripts/update-submodules.sh`, commit ("Point submodules to merged docs"), push;
   `./scripts/check-submodules.sh` must now pass.
4. Merge the main pull request last. (a-submodule-pins)
5. The bot needs the one-time GitHub App setup in `docs/github-setup.md` (variable `MUSICUS_BOT_CLIENT_ID`, secret
   `MUSICUS_BOT_PRIVATE_KEY`). Changes to `update-submodules.yml` only take effect after merging into `main`
   (`issue_comment` runs the default branch's file); test with Actions → `Update submodules` → `Run workflow`,
   branch `main`, input `pr`.

### Publish/test the user guide site
Site: GitHub Pages of the main repository (`https://<owner>.github.io/Musicus/`), workflow `User guide site`
(`.github/workflows/user-guide-pages.yml`), logic in `scripts/build-guide-site.sh` (a-guide-site).
1. Release: the last step of the release pipeline (`release.yml` job `site`) calls the workflow (`workflow_call`,
   input `site-artifact`) with the site its stage 1 built from all published releases plus the new one; earlier test
   builds disappear. Publishing a GitHub release by hand no longer deploys anything (no `release` trigger). The guide of a release is the `docs/user-guide` commit pinned
   in its tag, so update the guide (and its pointer) before tagging.
   The workspace `version` in `Cargo.toml` (raised on the release branch, `RELEASING.md`) is the version on the guide's
   title page, in `GET /api/version` and `--version` (a-version-source); the script warns on a tag ≠ workspace version.
   Docs built by hand: `docs/user-guide/build.sh [version]`, `docs/technical/build.sh [version]` (without argument:
   `$MUSICUS_VERSION`, else the workspace version, else "dev").
2. Test build: Actions → `User guide site` → `Run workflow`, branch `main`, input `ref` = branch/tag/commit to
   preview (empty = the selected branch). Result: all releases plus `/test-<ref>/`, marked "test build".
3. Locally (needs `typst` 0.15.x, `python3`): `./scripts/build-guide-site.sh --out /tmp/site [--preview HEAD]`
   builds every local tag (or `--releases FILE`, one tag per line); then `python3 -m http.server -d /tmp/site`.
   Without tags, create temporary local ones (`git tag v0.1.0 <commit>`; delete them afterwards, do not push).
4. One-time repository setup: Settings → Pages → Source `GitHub Actions`; Settings → Environments →
   `github-pages` → deployment branches and tags: `main` (the release pipeline runs on pushes to main and on
   manual runs from main) and any branch manual test builds start from. Details: `docs/github-setup.md`.
5. New Typst version: change `TYPST_VERSION`/`TYPST_SHA256` in the workflow (sha256 from the release asset) and
   the version in `docs/user-guide/README.md`; run the script locally over all tags first.

### Add or change a UI text (d-i18n-fluent, a-i18n-keys)
1. Add the English message to `crates/musicus-i18n/locales/en/<area>.ftl` (id `<area>-<page>-<what>`, comment for translators; counts as `{ $count -> [one] … *[other] … }`); terms from `docs/agents/glossary.md`.
2. Use it as `musicus_i18n::t::<id with _>(args)`; a missing message or wrong argument does not compile.
3. Add the German draft to `locales/de/<area>.ftl` (same id, same file; glossary terms, "du"; a-i18n-german). `cargo test -p musicus-i18n` fails for unused messages, a German message missing (`COMPLETE`), other arguments, obsolete ids, a message in another file than its English source, and plural variants that are not CLDR categories. Changed meaning = new id (and a new German draft).
4. Tell the maintainer which German messages are new (German is a draft until reviewed; `scripts/i18n-review.py`).

### Add a language
1. `crates/musicus-i18n/locales/<lang>/*.ftl` (same files and ids as `en`); `LANGUAGES` and negotiation pick it up.
2. Its CLDR plural categories in `tests/catalogue.rs::PLURAL_CATEGORIES`; add it to `COMPLETE` once it must ship complete.
3. Its key names (`key-name-<code>`) and the words of `part-name-with-key` become search spellings/connectors: add a
   preview update whose additive part is `DELETE FROM part_fts;` and update `part_index::tests::INDEXED_SPELLINGS` (the test fails until then).
4. Glossary column, e2e run like `e2e/german.mjs`, requirement R-I18N-* checks, docs (`status.md`, `13-i18n.typ`).

### Change the schema
Add a preview update `crates/musicus-server/migrations/preview/NNNN-<topic>.sql` (next free number after the existing previews, 0011 now; lower-case topic) — d-update-model,
a-preview-migrations, a-update-runner. Format (`crates/musicus-server/src/updates/file.rs`): header comments, then
`-- @additive` (new tables/columns nullable or with default, copies into them), optional `-- @rebuild` (refill the new
structures from the old ones; runs after an older program used the database), optional `-- @removal` (drops, the old
half of a rename; runs only when no release in the window needs it — never on development databases, only after the
release merge). Removals never go into the additive part; access restrictions move the item out of what older releases
read (R-OPS-017). While a removal is pending, code writes old and new structures and must tolerate the old ones being
gone. Never edit a merged preview (correction = new preview); never add `migrations/X.Y.0.sql` outside a release pull
request. Then run `./scripts/db-schema.sh` and commit the regenerated `crates/musicus-server/schema.sql` with the preview
(`cargo test` fails while it is out of date). Update `data-model.md`, queries, tests (`tests/updates.rs` shows how to test a
part with test programs). CI (workflow `Database`, a-db-check): job `db-check` fails when a file is added under
`migrations/` outside `preview/` (incl. `releases.txt`), when a file of `migrations/` that is on `main` is modified,
renamed or deleted, when `schema.sql` is out of date or changed without a new preview, or when the new preview's number
is not higher than every preview on `main` (renumber after another branch merged first); branches `release/X.Y.Z` are
exempt except for the schema comparison. Job `db-change` labels the pull request `db-change` and comments that the next
release must be a minor (never fails). Debug and plain `--release` builds apply previews;
only builds with `MUSICUS_RELEASE_BUILD=1` refuse them. A local demo database in an unclear state (some previews of a
merged release missing) is refused with a message: move the data directory away.

### Release
Step by step for the maintainer: `RELEASING.md` (repository root; kinds and what they may contain, `release/<version>`
branch, version bump, `scripts/release-migrations.py`, checklist incl. "patch: no new stored values", release pull
request with a `## Release notes` section → dry run → merge → real run → check, running it again, checking the docs tags
(R-OPS-036), troubleshooting). Pipeline: `.github/workflows/release.yml` (a-release-pipeline, R-OPS-049): dry run on every
push to a `release/X.Y.Z` branch (needs its open pull request; checks show there, other pull requests start nothing); the push of its merge to main starts the real run; `workflow_dispatch` (`version`, `ref`,
`publish`) runs it again from main. Stage 1 (expiring artifacts): `checks`, `lint`, `web`, `program` ×5 platforms,
`image`, `docs`, `assets` → gate `stage-1`; stage 2: `images` → `tag` → `github-release` → `site`. Locally:
```sh
python3 scripts/release-check.py --branch "$(git branch --show-current)" origin/main   # version + R-OPS-040 (on release/X.Y.Z)
python3 scripts/release-check.py --title-pages X.Y.Z docs/user-guide/out/*.html docs/user-guide/out/*.pdf docs/technical/out/*.{html,pdf}
python3 scripts/smoke-program.py --program target/release/musicus-server --version X.Y.Z   # built-in client; `--web dir -- --web-dir <dir>` otherwise
python3 scripts/package-program.py archive --program target/release/musicus-server --version X.Y.Z --platform linux-x86_64 --out dist && python3 scripts/package-program.py sums dist
python3 scripts/release-notes.py --version X.Y.Z --body pr-description.md
docker buildx build --target release --build-context program=<dir with amd64/musicus-server> --platform linux/amd64 --load -t musicus:release .
```
Pitfalls: a program built on a newer glibc than Debian 12 (2.36) does not start in the image (the pipeline builds on
ubuntu-22.04; locally build inside `rust:1.97-bookworm`); multi-platform `--output type=oci` needs a `docker-container`
buildx builder; `MUSICUS_RELEASE_BUILD=1` without `--features embed-web` fails in `build.rs`, with previews the program
refuses to start. Changes to `rust.yml` jobs: keep the job `lint` of `release.yml` in step. `RELEASING.md` is for
humans: it never refers to `AGENTS.md` or `docs/agents`; its step 6 repeats the commands of `AGENTS.md` § Commands —
change both together. The version on the title pages comes from the workspace version (a-version-source).

### Add/modify a requirement-driven feature
1. Read the requirement + its links (`refines`, `depends_on`, `constrains`, `conflicts_with`) in `docs/requirements`.
2. Implement; add tests.
3. In `docs/requirements` (submodule): set `status` (`implemented`/`verified`), add `implemented_by`/`verified_by`
   (code and tests; never database update files under `crates/musicus-server/migrations/` except `releases.txt` —
   previews are merged and deleted at a release, `validate.py` rejects them; table-related requirements reference
   the schema snapshot `crates/musicus-server/schema.sql` instead, a-db-check); new behaviour ⇒ new requirement file (next free number in the area; `refines` a feature); `uv run tools/validate.py`; commit in the submodule, push it, then commit the pointer in the main repo.
4. Update `docs/agents/*` (this folder) and run `check.py`.

### Change a design decision
Read the entry in `decisions.md` (Serves / If you change it). If a served requirement would no longer hold: ask the user; update the requirement first. Update `decisions.md`; for `d-*` also `docs/technical/chapters/10-decisions.typ` (or tell the user).

## Definition of done (every change)

- [ ] `cargo test`, `cargo clippy --all-targets`, `cargo fmt --check` clean; web clippy clean if client touched.
- [ ] Client changes: built with `dx build` and exercised (ideally `e2e/smoke.mjs`); `scripts/check-ui-literals.py` clean; new texts in the catalogue (`e2e/pseudo.mjs` for new pages).
- [ ] `docs/agents/*` describe the new state; `python3 docs/agents/check.py --since <base>` passes.
- [ ] Requirements updated and validated; docs submodule pull requests merged first, then pointers updated with
      `./scripts/update-submodules.sh` so that `./scripts/check-submodules.sh` passes.
- [ ] User-visible change: `docs/user-guide` text and screenshots updated (or user told).
- [ ] Commit message explains why; mentions requirement IDs.
