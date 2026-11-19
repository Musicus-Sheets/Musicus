# Workflows, commands, pitfalls

## Build & test

```sh
cargo build                       # api + server (web excluded from default-members)
cargo test                        # 29 api unit, 17 server unit, 13 API integration tests
cargo run --release -p musicus-server --example layout_report -- a.pdf …   # system detection per page (regressions)
cargo clippy --all-targets        # must be warning-free
cargo fmt --check                 # rustfmt.toml: max_width 120
cargo clippy -p musicus-web --target wasm32-unknown-unknown
./scripts/fetch-pdfjs.sh          # once (again after changes of the script); populates crates/musicus-web/public/vendor/pdfjs (gitignored)
dx build --platform web --package musicus-web            # debug → target/dx/musicus-web/debug/web/public
dx bundle --platform web --package musicus-web --release # release (needs wasm-opt)
cargo run -p musicus-server -- serve --data-dir /tmp/m --web-dir target/dx/musicus-web/debug/web/public --insecure-cookies
node e2e/smoke.mjs <admin-invite-url-from-log> some.pdf /tmp/shots   # fresh data dir required; BROWSER=firefox for Firefox
python3 docs/agents/check.py [--since <ref>]
uv run docs/requirements/tools/validate.py
docker compose up -d --build
```

Toolchain: Rust ≥ 1.88 (edition 2024, let-chains used), target `wasm32-unknown-unknown`, `dx` 0.7.10 (must match dioxus crate version), wasm-bindgen CLI version must equal the `wasm-bindgen` crate version in Cargo.lock (dx downloads it).

## Environment pitfalls (seen in practice)

- No local wasm toolchain? Build an image `FROM rust:1.97-bookworm` + `rustup target add wasm32-unknown-unknown` +
  `cargo install dioxus-cli --version 0.7.10`, mount the repo, set `CARGO_TARGET_DIR=/src/target/docker`, and run the
  web clippy / `dx build` inside it; serve the result with a host-built server (`--web-dir target/docker/dx/...`).
- Sharpness tests: Playwright's `deviceScaleFactor` (CDP DSF emulation) lays out in CSS pixels and scales the result, so
  fractional CSS sizes always look resampled there; use a real factor (`chromium.launch({args:
  ['--force-device-scale-factor=2']})` + `viewport: null`) and compare `devicePixelContentBoxSize` with the canvas size
  (a-device-pixels). Screenshot comparisons: crop the full device-pixel screenshot, a fractional `clip` resamples.
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
3. After the docs pull requests are merged into their `main`: `./scripts/update-submodules.sh`, commit
   ("Point submodules to merged docs"), push. `./scripts/check-submodules.sh` must now pass.
4. Merge the main pull request last. (a-submodule-pins)

### Publish/test the user guide site
Site: GitHub Pages of the main repository (`https://<owner>.github.io/Musicus/`), workflow `User guide site`
(`.github/workflows/user-guide-pages.yml`), logic in `scripts/build-guide-site.sh` (a-guide-site).
1. Release: publishing a GitHub release (not a prerelease, not a draft) rebuilds the whole site from all release
   tags and deploys it; earlier test builds disappear. The guide of a release is the `docs/user-guide` commit pinned
   in its tag, so update the guide (and its pointer) before tagging.
2. Test build: Actions → `User guide site` → `Run workflow`, branch `main`, input `ref` = branch/tag/commit to
   preview (empty = the selected branch). Result: all releases plus `/test-<ref>/`, marked "test build".
3. Locally (needs `typst` 0.15.x, `python3`): `./scripts/build-guide-site.sh --out /tmp/site [--preview HEAD]`
   builds every local tag (or `--releases FILE`, one tag per line); then `python3 -m http.server -d /tmp/site`.
   Without tags, create temporary local ones (`git tag v0.1.0 <commit>`; delete them afterwards, do not push).
4. One-time repository setup: Settings → Pages → Source `GitHub Actions`; Settings → Environments →
   `github-pages` → deployment branches and tags: add the release tag pattern (e.g. `v*`) and any branch manual
   runs start from (the default branch is allowed by default). Otherwise deploys from release tags are rejected.
5. New Typst version: change `TYPST_VERSION`/`TYPST_SHA256` in the workflow (sha256 from the release asset) and
   the version in `docs/user-guide/README.md`; run the script locally over all tags first.

### Change the schema
New `crates/musicus-server/migrations/000N_<desc>.sql` (never edit applied ones); update `data-model.md`; update queries; tests.

### Add/modify a requirement-driven feature
1. Read the requirement + its links (`refines`, `depends_on`, `constrains`, `conflicts_with`) in `docs/requirements`.
2. Implement; add tests.
3. In `docs/requirements` (submodule): set `status` (`implemented`/`verified`), add `implemented_by`/`verified_by`; new behaviour ⇒ new requirement file (next free number in the area; `refines` a feature); `uv run tools/validate.py`; commit in the submodule, push it, then commit the pointer in the main repo.
4. Update `docs/agents/*` (this folder) and run `check.py`.

### Change a design decision
Read the entry in `decisions.md` (Serves / If you change it). If a served requirement would no longer hold: ask the user; update the requirement first. Update `decisions.md`; for `d-*` also `docs/technical/chapters/10-decisions.typ` (or tell the user).

## Definition of done (every change)

- [ ] `cargo test`, `cargo clippy --all-targets`, `cargo fmt --check` clean; web clippy clean if client touched.
- [ ] Client changes: built with `dx build` and exercised (ideally `e2e/smoke.mjs`).
- [ ] `docs/agents/*` describe the new state; `python3 docs/agents/check.py --since <base>` passes.
- [ ] Requirements updated and validated; docs submodule pull requests merged first, then pointers updated with
      `./scripts/update-submodules.sh` so that `./scripts/check-submodules.sh` passes.
- [ ] User-visible change: `docs/user-guide` text and screenshots updated (or user told).
- [ ] Commit message explains why; mentions requirement IDs.
