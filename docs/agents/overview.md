# Overview

State: first implementation of concept A (crate version 0.1.0). Concept doc lives in the user's
Claude Docs ("Musicus – Conceptual Design"); requirements in `docs/requirements` are authoritative.

## Purpose (goals)

Ensemble library of PDF parts (G-001); access by group→instrument grants (G-002); private, named,
non-destructive annotation sets (G-003); reliable on-stage reading incl. whole-ensemble bursts (G-004);
runs on Raspberry Pi / 1 vCPU 1 GB (G-005); security & privacy (G-006); later offline app + MusicXML
(G-007, proposed); Rust + Dioxus (G-008, F-TECH-001).

## Components

```
browser ──HTTPS(optional Caddy)──▶ musicus-server (1 process) ──▶ data dir: musicus.sqlite (+wal) , blobs/ab/<sha256>, tmp/
  musicus-web (WASM, Dioxus 0.7)        Axum 0.8 + sqlx 0.9 (SQLite) + tower-http + hayro
  + js/bridge.js + pdf.js legacy        serves /api/* and the built web client (SPA fallback to index.html)
musicus-api (lib) compiled into both: JSON DTOs + binary ink format
```

| Component | Path | Builds with | Role |
| --- | --- | --- | --- |
| musicus-api | `crates/musicus-api` | cargo (any target) | Contract: DTOs (`dto.rs`), annotation format + merge (`ink.rs`). No I/O. |
| musicus-server | `crates/musicus-server` | cargo | API, auth, ACL, storage, CLI (`serve`, `create-admin`, `backup`, `healthcheck`). |
| musicus-web | `crates/musicus-web` | `dx` (wasm32 only; excluded from workspace `default-members`) | UI. Talks only REST. |
| e2e | `e2e/smoke.mjs` | node + Playwright | Browser smoke test against a running server. |
| deployment | `Dockerfile`, `compose.yaml`, `deploy/Caddyfile`, `.env.example` | docker | Distroless image; optional Caddy HTTPS profile. |
| prototypes | `prototypes/` | separate cargo projects | Throw-away benchmarks from the concept phase. Not product code; do not extend. |
| docs | `docs/technical` (submodule, Typst), `docs/requirements` (submodule, Markdown+YAML), `docs/agents` (this) | — | — |

## Request lifecycle (server)

1. Axum router (`routes::app`) → middleware: security headers (R-SEC-005), TraceLayer, body limits.
2. `/api/*` → handler; anything else → `ServeDir(web_dir)` with fallback `index.html` (client routes).
3. Handler extracts `State<AppState>` (`Arc<App>`), `AuthUser` (auth + CSRF + session lookup), path/body.
4. Role check (`require_librarian`/`require_admin`) and/or ACL via `part_visible!()` SQL fragment.
5. SQL via sqlx: reads on `app.db.read` pool, writes (in a tx when >1 statement) on `app.db.write` (1 connection).
6. Response: `musicus-api` type as JSON, or bytes (files, ink pages). Errors: `AppError` → status + `ApiError` JSON.

## Data flow per use case (pointers)

- Login: client `pages/login.rs` → `POST /api/auth/login` → `auth::verify_password` (semaphore, Argon2) → session row → cookie.
- Library: `GET /api/scores?q=` → `library::list_scores` (FTS5 + `part_visible!`) → `with_parts`.
- Programme prefetch: `GET /api/programmes/{id}` → client collects `/api/files/<sha>` URLs → `bridge.js prefetch` (in-memory Map).
- Reading: `bridge.js renderPage` (pdf.js from in-memory bytes) ; annotations `GET/PUT /api/annotation-sets/{id}/pages/{n}`.
- Upload: `PUT /api/parts/{id}/file` raw body → `BlobStore::receive` (hash+limit) → sniff → hayro (page count, thumbnail) → commit → `files` row → `part_files` version.

## Global invariants (break none without changing the linked requirements)

| ID | Invariant | Requirements | Decision |
| --- | --- | --- | --- |
| INV-1 | Read permission is decided only by `access::part_visible!()` (librarian/admin, instrument grant via group, score grant via group). Every list/load of parts, scores, programmes, files, annotations embeds it. | R-ACL-001, R-LIB-005, R-PROG-002, R-FILE-005, R-ANN-015 | d-access |
| INV-2 | Denied read ⇒ 404 (`AppError::NotFound`), indistinguishable from missing. Role violations on management endpoints ⇒ 403. | R-ACL-002, R-ACC-009 | d-access |
| INV-3 | No permission/session cache in memory; every request re-reads DB. Grant/membership changes apply on next request. | R-ACL-003, R-ACC-008 | d-access, d-sessions |
| INV-4 | Blobs immutable, named by SHA-256, written tmp→fsync→rename. Never delete/modify. | R-FILE-004, R-FILE-002 | d-blobs |
| INV-5 | All SQL is static (`&'static str`, `concat!`, or `AssertSqlSafe` over constants only). User input only via bind parameters. Id lists via JSON + `json_each`. | R-SEC-006 | d-static-sql |
| INV-6 | Exactly one write connection; reads use the read-only pool; keep write transactions short. | R-PERF-006, R-TECH-001 | d-sqlite |
| INV-7 | Annotation objects are immutable and have unique random u64 ids within a page; edits = remove+add. | R-ANN-012 | d-optimistic |
| INV-8 | Annotation page save requires `If-Match` revision; mismatch ⇒ 409 `PageConflict`; client merges. | R-ANN-012 | d-optimistic |
| INV-9 | Annotations visible only to owner (`user_id = AuthUser.id` in every query) AND only while the owner can read the part. | R-ANN-014, R-ANN-015 | — |
| INV-10 | Web client persists nothing (no localStorage/IndexedDB/Cache API/service worker); sheets in JS memory Map. | R-SEC-001, R-PROG-003 | d-no-client-storage |
| INV-11 | Passwords: Argon2id via `auth::hash_password/verify_password` only (semaphore). Tokens: store SHA-256 only. | R-SEC-002, R-PERF-002 | d-hashing, d-sessions |
| INV-12 | Cookie-authenticated non-GET/HEAD/OPTIONS requests require header `x-musicus-csrf`. Client `api.rs` always sends it. | R-SEC-004 | d-csrf |
| INV-13 | Annotation sets are bound to a `files.id` (file version), not just a part. | R-ANN-016 | d-blobs |
| INV-14 | Every type crossing the HTTP boundary is defined in `musicus-api`. | R-TECH-003 | d-shared-types |
