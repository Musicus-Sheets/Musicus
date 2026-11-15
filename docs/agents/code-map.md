# Code map

Every product file. First column is parsed by `check.py` (must list every file matched by its patterns).
Columns: path | purpose | key symbols | requirements implemented | decisions.

## Workspace / build

| Path | Purpose | Key symbols | Requirements | Decisions |
| --- | --- | --- | --- | --- |
| `Cargo.toml` | Workspace: members api/server/web; `default-members` excludes web (wasm-only); profiles `release`, `wasm-release` | — | R-TECH-001, R-TECH-002 | a-workspace-members |
| `rustfmt.toml` | `max_width = 120`, `use_small_heuristics = "Max"` | — | — | — |
| `crates/musicus-api/Cargo.toml` | Deps: serde, postcard only | — | R-TECH-003 | d-shared-types |
| `crates/musicus-server/Cargo.toml` | Deps: axum 0.8, axum-server (rustls, ring), sqlx 0.9 sqlite, tower-http, argon2, hayro, zstd, clap, tracing | — | R-TECH-001 | d-sqlite |
| `crates/musicus-web/Cargo.toml` | Deps: dioxus 0.7.10 (web, router), gloo-net, gloo-timers, wasm-bindgen, web-sys | — | R-TECH-002 | — |
| `crates/musicus-web/Dioxus.toml` | `dx` app name/title | — | — | — |
| `scripts/fetch-pdfjs.sh` | Downloads pinned pdfjs-dist 6.5.438 (sha512-checked), copies **legacy** build to `crates/musicus-web/public/vendor/pdfjs/` (gitignored) | `VERSION`, `INTEGRITY` | R-READ-006 | d-pdf-rendering |

## musicus-api (`crates/musicus-api/src`)

| Path | Purpose | Key symbols | Requirements | Decisions |
| --- | --- | --- | --- | --- |
| `crates/musicus-api/src/lib.rs` | Re-exports `dto::*`, module `ink` | — | R-TECH-003 | d-shared-types |
| `crates/musicus-api/src/dto.rs` | All JSON bodies + constants | `Role`, `ApiError`, `CSRF_HEADER`, `SESSION_COOKIE`, `UpdateAnnotationSet` (`double_option`), `IdList`, `PageConflict` | R-TECH-003, R-SEC-004 | d-shared-types |
| `crates/musicus-api/src/ink.rs` | Annotation model, `encode`/`decode` (version byte + postcard, limits), `merge` | `PageLayer`, `InkObject`, `Stroke::from_points/points`, `StampKind`, `COORD_MAX`, `MAX_*`, `merge` | R-ANN-002, R-ANN-005, R-ANN-006, R-ANN-007, R-ANN-012 | d-ink-format, d-optimistic |

## musicus-server (`crates/musicus-server`)

| Path | Purpose | Key symbols | Requirements | Decisions |
| --- | --- | --- | --- | --- |
| `crates/musicus-server/migrations/0001_init.sql` | Full schema v1 incl. FTS5 + triggers | see `data-model.md` | R-LIB-003, R-LIB-004, R-PERF-005, R-OPS-004 | d-sqlite |
| `crates/musicus-server/src/main.rs` | CLI dispatch; `serve` (plain or rustls, graceful shutdown); `create-admin`; `backup`; `healthcheck` (raw HTTP/1.0) | `serve`, `healthcheck`, `shutdown_signal` | R-ACC-003, R-OPS-001, R-OPS-005, R-OPS-006, R-OPS-007, R-OPS-008 | d-monolith, d-distroless |
| `crates/musicus-server/src/lib.rs` | `open_state`, `bootstrap_admin`, `anyhow_like` boxed error | `open_state`, `bootstrap_admin` | R-ACC-003 | d-invites |
| `crates/musicus-server/src/config.rs` | clap CLI + `MUSICUS_*` env | `Cli`, `Command`, `ServeArgs`, `DataArgs`, `ClientCache` | R-OPS-002, R-FILE-007 | — |
| `crates/musicus-server/src/state.rs` | `App` (db, blobs, cache, settings, limiter, hashing semaphore), `Settings` | `App::new`, `AppState` | R-PERF-002 | d-hashing |
| `crates/musicus-server/src/db.rs` | Pools (1 writer + 2–8 readers), pragmas, migrations, `VACUUM INTO` backup, `now()`, `json_ids()` | `Db::open`, `Db::backup_to` | R-OPS-004, R-OPS-005, R-PERF-006 | d-sqlite, d-static-sql |
| `crates/musicus-server/src/error.rs` | `AppError` → status + `ApiError`; sqlx unique/FK → 409 (incl. `ON DELETE RESTRICT`, reported by SQLite as code 1811: `is_restrict_violation`); `required`/`optional` validators | `AppError`, `ApiResult` | R-SEC-008, R-LIB-003 | — |
| `crates/musicus-server/src/auth.rs` | Argon2 hash/verify (semaphore, dummy hash), tokens, sessions, cookie, `AuthUser` extractor (bearer/cookie, CSRF, sliding expiry hourly), `ClientIp`, `LoginLimiter` | `AuthUser`, `hash_password`, `verify_password`, `create_session`, `session_cookie`, `LoginLimiter` | R-ACC-004, R-ACC-005, R-ACC-009, R-SEC-002, R-SEC-003, R-SEC-004, R-PERF-002 | d-sessions, d-csrf, d-hashing |
| `crates/musicus-server/src/access.rs` | `part_visible!()` macro (SQL fragment, binds ?1 user, ?2 librarian flag), `can_read_part`, `ensure_part_readable` | `part_visible!`, `librarian_flag` | R-ACL-001, R-ACL-002, R-ACL-003, R-PERF-005 | d-access |
| `crates/musicus-server/src/blobs.rs` | Content-addressed store: `receive` (stream→tmp, hash, limit), `commit` (fsync+rename, dedupe), `put_bytes`, `path` | `BlobStore`, `PendingBlob`, `UploadError`, `is_sha256_hex` | R-FILE-001, R-FILE-004 | d-blobs |
| `crates/musicus-server/src/cache.rs` | LRU RAM cache sha→`Bytes`; skips files > capacity/4 | `FileCache` | R-PERF-003, R-PERF-004 | a-ram-cache |
| `crates/musicus-server/src/processors.rs` | `sniff_mime` (`%PDF-` in first 1 KiB), `process_pdf` (hayro page count + 240 px PNG thumbnail; best effort) | `process_pdf`, `PDF_MIME` | R-FILE-001, R-FILE-003 | d-pdf-rendering |
| `crates/musicus-server/src/routes/mod.rs` | Router table, middleware, `/api/health`, SPA fallback (`ServeDir` + precompressed br/gz) | `api()`, `app()`, `health` | R-OPS-001, R-OPS-007, R-SEC-005 | d-monolith |
| `crates/musicus-server/src/routes/auth.rs` | Login/logout/me, invites, password change, sessions list/revoke | `login`, `accept_invite`, `change_password` | R-ACC-002, R-ACC-004, R-ACC-006, R-ACC-007, R-SEC-003 | d-invites, d-sessions |
| `crates/musicus-server/src/routes/users.rs` | Admin user CRUD, invites | `insert_user`, `issue_invite` | R-ACC-001, R-ACC-008, R-ACC-010 | d-invites |
| `crates/musicus-server/src/routes/groups.rs` | Instruments, groups, members, instrument/score grants (`replace_links`) | `replace_links`, `Link` | R-ACL-004, R-LIB-003 | d-static-sql |
| `crates/musicus-server/src/routes/library.rs` | Scores, parts, upload + versions, search; helpers `visible_parts`, `with_parts`, `scores_by_ids`, `fts_query` | `upload_file`, `store_upload`, `list_scores` | R-LIB-001, R-LIB-002, R-LIB-004, R-LIB-005, R-FILE-001, R-FILE-002 | d-access, d-blobs |
| `crates/musicus-server/src/routes/files.rs` | File/thumbnail delivery: ACL, ETag/304, Range→`ServeFile`, else RAM cache; Cache-Control by setting | `readable_file`, `serve_blob` | R-FILE-005, R-FILE-006, R-FILE-007, R-PERF-003 | a-file-acl-any-part, a-ram-cache |
| `crates/musicus-server/src/routes/programmes.rs` | Programmes CRUD + items; member visibility | `load`, `set_items`, `valid_date` | R-PROG-001, R-PROG-002 | — |
| `crates/musicus-server/src/routes/annotations.rs` | Sets CRUD/duplicate/default/programme link/migrate; page GET (204 if unsaved) / PUT (If-Match, 409, zstd) | `own_set`, `put_page`, `get_page`, `migrate` | R-ANN-006, R-ANN-008, R-ANN-012, R-ANN-013, R-ANN-014, R-ANN-015, R-ANN-016 | d-optimistic, d-204 |
| `crates/musicus-server/tests/api.rs` | Integration tests via `tower::ServiceExt::oneshot` on a temp data dir | `Server`, `library()` fixture, 7 tests | (see `verified_by` in requirements) | — |

## musicus-web (`crates/musicus-web`)

| Path | Purpose | Key symbols | Requirements | Decisions |
| --- | --- | --- | --- | --- |
| `crates/musicus-web/src/main.rs` | Launch, `Route` enum, `Session` context (`user`, `checked`), `/api/auth/me` on start | `Route`, `Session`, `session()` | R-TECH-002 | — |
| `crates/musicus-web/src/ui.rs` | `Shell` (nav: Library, Programmes, Instruments; Groups for librarian/admin; Users for admin; login guard), role helpers `current_role`/`can_manage`/`is_admin`, `ErrorBox`, `Status`, `NoAccess`, formatting, `confirm`/`prompt`, `none_if_empty`/`tags_from` | `Shell`, `can_manage`, `is_admin` | R-ACC-009, R-ACC-013, R-ACC-014 | d-inline-management |
| `crates/musicus-web/src/api.rs` | HTTP helpers (always CSRF header), 401 → full reload to `/login`, `get_page` (204 = Missing), `put_page` (If-Match, 409 = Conflict), `upload` | `get`, `post`, `put_page`, `get_page`, `SaveOutcome`, `PageBytes` | R-SEC-004, R-ANN-013 | d-csrf, a-unauthorized-reload |
| `crates/musicus-web/src/bridge.rs` | Typed wasm-bindgen wrappers for `js/bridge.js`; `prefetch` starts eagerly | `prefetch`, `render_page`, `page_count`, `retain_only`, `set_pending`, `clear_pending`, `keep_awake` | R-PROG-003, R-ANN-011 | a-eager-prefetch |
| `crates/musicus-web/js/bridge.js` | pdf.js loading/rendering (off-screen canvas, cancel older), in-memory bytes Map, pending pages flushed on `pagehide` with keepalive, wake lock, fullscreen, clipboard | `prefetch`, `renderPage`, `setPending`, `keepAwake` | R-READ-001, R-READ-003, R-READ-004, R-READ-006, R-PROG-003, R-PROG-004, R-ANN-011, R-SEC-001, R-PERF-007 | d-pdf-rendering, d-no-client-storage, d-reader-store, a-wasm-view-copy |
| `crates/musicus-web/src/ink.rs` | Canvas drawing of layers (highlighter first), `PageBox` coordinate mapping, eraser hit test, `new_id` | `PageBox`, `draw_layer`, `hit`, `segment` | R-ANN-001, R-ANN-002, R-ANN-005 | d-ink-format |
| `crates/musicus-web/src/pages/mod.rs` | Module list | — | — | — |
| `crates/musicus-web/src/pages/login.rs` | Login, invite acceptance | `Login`, `Invite` | R-ACC-002, R-ACC-004 | d-invites |
| `crates/musicus-web/src/pages/account.rs` | Password change, device list/revoke | `Account` | R-ACC-006, R-ACC-007 | — |
| `crates/musicus-web/src/pages/library.rs` | Library list + search (150 ms debounce), `ScoreCard` (L: `Edit`, missing-PDF tags), `PartTile` (L: `Edit` under the tile), `+ Add piece` (L), `FOCUS_SCORE` highlight, `part_route` | `Library`, `ScoreCard`, `FOCUS_SCORE` | R-LIB-004, R-LIB-005, R-LIB-006, R-ACC-013, R-ACC-014 | d-inline-management |
| `crates/musicus-web/src/pages/programmes.rs` | Programme list (L: `+ New programme`); view with prefetch (3 at a time, progress, `retain_only`; L: `Edit`) | `ProgrammeView`, `programme_urls` | R-PROG-002, R-PROG-003, R-PROG-004, R-ACC-013 | a-eager-prefetch, d-no-client-storage |
| `crates/musicus-web/src/pages/forms.rs` | Shared editing pieces: `ScoreDraft`/`ScoreFields`, `InstrumentChecks`, `PartEditor` (fields, upload/replace, versions, delete), `NewPartForm` (create + background upload), `AddedParts`, `upload_pdf` | `PartEditor`, `NewPartForm`, `Upload` | R-LIB-001, R-LIB-002, R-FILE-001, R-FILE-002, R-LIB-009 | d-inline-management |
| `crates/musicus-web/src/pages/score_edit.rs` | `NewScore` (guided flow: details → parts with PDFs → library), `EditScore` (details, parts, add part, delete piece), `EditPart` | `NewScore`, `EditScore`, `EditPart`, `Steps` | R-LIB-001, R-LIB-002, R-LIB-009, R-ACC-013 | d-inline-management |
| `crates/musicus-web/src/pages/programme_edit.rs` | `NewProgramme`/`EditProgramme` over `ProgrammeEditor` (title, date, notes, ordered pieces, delete) | `ProgrammeEditor` | R-PROG-001, R-ACC-013 | d-inline-management |
| `crates/musicus-web/src/pages/instruments.rs` | Instrument list by family for everyone; L: add, inline edit, delete (409 message) | `Instruments`, `InstrumentRow`, `by_family` | R-LIB-003, R-LIB-008, R-ACC-013 | d-inline-management |
| `crates/musicus-web/src/pages/groups.rs` | Groups tab: members (admin), instrument and score grants (librarian/admin) | `Groups`, `GroupEditor` | R-ACL-004 | — |
| `crates/musicus-web/src/pages/users.rs` | Users tab (admin): create with invite link, roles, disable/enable, new invite | `Users`, `UserRow`, `invite_link` | R-ACC-001, R-ACC-010 | d-invites |
| `crates/musicus-web/src/pages/viewer.rs` | Reader: rendering, annotation `Store` (outside signals), save pipeline + merge, pointer input, tools, sets menu, page/programme navigation, keyboard via `document::eval` | `Reader`, `Store`, `PageState`, `load_page`, `save_page`, `sync_pending`, `neighbours`, `choose_set` | R-READ-001, R-READ-002, R-READ-004, R-READ-005, R-ANN-001, R-ANN-002, R-ANN-003, R-ANN-004, R-ANN-008, R-ANN-009, R-ANN-010, R-ANN-011, R-ANN-012, R-ANN-016 | d-reader-store, d-optimistic, a-live-ink |
| `crates/musicus-web/assets/main.css` | All styles; tablet-first, dark mode, reader layout | — | R-READ-001 | — |

## Tests, deployment, tooling

| Path | Purpose | Key symbols | Requirements | Decisions |
| --- | --- | --- | --- | --- |
| `e2e/smoke.mjs` | Playwright end-to-end flow (40 checks): admin, librarian and member views incl. guided flow, part/piece/programme/instrument editing, role-gated controls, reader | `check()` | many (`verified_by`) | — |
| `e2e/package.json` | Declares playwright dev dependency | — | — | — |
| `e2e/.gitignore` | Ignores node_modules | — | — | — |
| `Dockerfile` | Stages dx → build (pdf.js, dx bundle, cargo) → distroless nonroot runtime; HEALTHCHECK via subcommand | `RUST_IMAGE`, `RUNTIME_IMAGE`, `DX_VERSION` args | R-OPS-003, R-SEC-007 | d-distroless |
| `.dockerignore` | Excludes target, data, prototypes, e2e, docs vendor | — | — | — |
| `compose.yaml` | Service `musicus` (volume `musicus-data`), profile `https` with Caddy | — | R-OPS-003, R-OPS-006 | — |
| `deploy/Caddyfile` | TLS + compression → `musicus:8080` | — | R-OPS-006 | — |
| `.env.example` | Compose settings template | — | R-OPS-002 | — |
| `AGENTS.md` | Mandatory agent instructions (entry point) | — | — | — |
| `CLAUDE.md` | Imports `AGENTS.md` for Claude Code | — | — | — |
| `.githooks/pre-commit` | Runs `docs/agents/check.py --staged` | — | — | — |
| `.github/workflows/agent-docs.yml` | CI merge gate: `check.py --since <PR base>` on pull requests and merge queue only (required status check `check`) | — | — | — |
| `docs/agents/check.py` | This folder's consistency checker | — | — | — |
