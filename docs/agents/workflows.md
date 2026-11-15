# Workflows, commands, pitfalls

## Build & test

```sh
cargo build                       # api + server (web excluded from default-members)
cargo test                        # 8 api unit, 10 server unit, 7 API integration tests
cargo clippy --all-targets        # must be warning-free
cargo fmt --check                 # rustfmt.toml: max_width 120
cargo clippy -p musicus-web --target wasm32-unknown-unknown
./scripts/fetch-pdfjs.sh          # once; populates crates/musicus-web/public/vendor/pdfjs (gitignored)
dx build --platform web --package musicus-web            # debug → target/dx/musicus-web/debug/web/public
dx bundle --platform web --package musicus-web --release # release (needs wasm-opt)
cargo run -p musicus-server -- serve --data-dir /tmp/m --web-dir target/dx/musicus-web/debug/web/public --insecure-cookies
node e2e/smoke.mjs <admin-invite-url-from-log> some.pdf /tmp/shots   # fresh data dir required
python3 docs/agents/check.py [--since <ref>]
uv run docs/requirements/tools/validate.py
docker compose up -d --build
```

Toolchain: Rust ≥ 1.88 (edition 2024, let-chains used), target `wasm32-unknown-unknown`, `dx` 0.7.10 (must match dioxus crate version), wasm-bindgen CLI version must equal the `wasm-bindgen` crate version in Cargo.lock (dx downloads it).

## Environment pitfalls (seen in practice)

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
