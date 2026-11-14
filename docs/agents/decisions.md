# Design decisions

`d-*`: also recorded for humans in `docs/technical/chapters/10-decisions.typ` (same label).
`a-*`: implementation-level decisions recorded only here.
Fields: **Decision** · **Why** · **Serves** (requirement IDs this decision exists for) · **If you change it** (what else
must change / what breaks). A decision may only be reversed if the served requirements stay fulfilled or are changed
with the user's consent.

### d-monolith
- Decision: one process = API + static web client + embedded SQLite + local blob store. No separate DB/object store/cache services.
- Why: small hardware and volunteer operators; measured 11 MB idle RSS; one binary/container; backup = db file + folder.
- Serves: G-005, F-PERF-001, F-OPS-001, R-OPS-001, R-PERF-001, R-TECH-001.
- If you change it: adding a service (Redis, Postgres, S3) needs user consent; concept variant "A-PG" (PostgreSQL in compose) was evaluated and deferred by the user. Keep `musicus-server` self-contained.

### d-sqlite
- Decision: SQLite in WAL mode via sqlx; `Db.write` pool has exactly 1 connection (the write queue), `Db.read` 2–8 read-only connections; `synchronous=NORMAL`, `foreign_keys=ON`, `busy_timeout=5s`.
- Why: single writer is SQLite's model; a pool of one serialises writes in-process instead of SQLITE_BUSY; WAL readers never block. Benchmarks: 2M parts/1M pages fine; 560–2800 saves/s.
- Serves: R-TECH-001, R-PERF-006, R-PERF-005.
- If you change it: never write on `db.read` (read-only, fails); keep write transactions short (they block all writes); long imports → batch in small transactions. Switching to PostgreSQL = rewrite of SQL dialect bits (`json_each`, FTS5, `INSERT OR IGNORE`, `RETURNING` ok) — user decision.

### d-static-sql
- Decision: all SQL is compile-time constant. Dynamic id lists are one JSON text param expanded with `json_each(?)` (`db::json_ids`). Shared fragment `part_visible!()` spliced with `concat!`. `routes/annotations.rs` uses `sqlx::AssertSqlSafe(format!("{SET_SELECT} …"))` — allowed only because all parts are constants.
- Why: sqlx 0.9 only accepts `&'static str` (or explicit `AssertSqlSafe`); makes injection impossible by construction.
- Serves: R-SEC-006.
- If you change it: never put user input into `AssertSqlSafe`/`format!` SQL. For variable filters use `(?n IS NULL OR …)` patterns as in `library::list_scores`.

### d-blobs
- Decision: files stored once at `blobs/<sha[0..2]>/<sha256>`, written via `tmp/` + fsync + rename, immutable, never deleted by the app. Parts reference files through versioned `part_files`; annotation sets reference a `files.id`. Annotation layers are NOT blobs (they live in `annotation_pages`).
- Why: immutable hash names → ETag = hash, dedupe, crash-safe, incremental backup, future offline sync by hash; big files out of SQLite WAL/backups; small, mutable, revisioned ink pages fit the DB.
- Serves: R-FILE-004, R-FILE-002, R-ANN-016, R-OFF-002 (future), F-OPS-002.
- If you change it: GC of unreferenced blobs is R-FILE-009 (proposed) and must respect backup consistency (grace period). Changing blob naming breaks ETags and existing data dirs.

### d-access
- Decision: one SQL fragment `part_visible!()` = librarian flag OR instrument grant via group membership OR score grant via group membership. Embedded in every query that lists/loads parts, scores, programmes, files (via `part_files`), annotations (via `ensure_part_readable`). No permission cache. Denied ⇒ 404.
- Why: ~25 µs per check at 200k parts; one definition so lists and point-loads agree; 404 hides library contents.
- Serves: R-ACL-001, R-ACL-002, R-ACL-003, R-PERF-005, R-LIB-005, R-PROG-002, R-FILE-005, R-ANN-015.
- If you change it: changing the rule = change R-ACL-001 first. New grant kinds (R-ACL-005/006, proposed) belong inside `part_visible!()`. Binds: `?1` user id, `?2` librarian flag (0/1); queries using it must keep these positions. Add an integration test for every new rule.

### d-sessions
- Decision: opaque 256-bit hex tokens; DB stores SHA-256 only (`sessions.token_hash`); lookup per request in `AuthUser`; sliding expiry (default 60 d) updated at most once per hour; no JWT.
- Why: immediate revocation (logout, password change, disable); no secrets usable from a DB leak; hourly touch avoids a DB write per read request (single writer).
- Serves: R-ACC-005, R-ACC-006, R-ACC-007, R-ACC-008, R-SEC-002.
- If you change it: bearer tokens for apps use the same table (`LoginRequest.bearer = true`). Keep token comparison by hash.

### d-csrf
- Decision: cookie `HttpOnly; SameSite=Lax; Secure` (Secure off only with `--insecure-cookies`); cookie-authenticated state-changing requests require header `x-musicus-csrf` (any value); bearer requests exempt.
- Why: custom headers cannot be sent cross-site without a CORS preflight, which is never allowed. No token storage needed.
- Serves: R-SEC-004.
- If you change it: never add permissive CORS. Every client request helper must send the header (`musicus-web/src/api.rs::builder`, `bridge.js` keepalive flush).

### d-invites
- Decision: admins never set passwords. Creating a user returns a one-time invite token (7 d); accepting sets the password, deletes all invites + sessions of the user, logs in. First start creates `admin` + logs an invite link. Reset = new invite.
- Why: nobody else knows a password; no default password; no SMTP needed.
- Serves: F-ACC-001, R-ACC-001, R-ACC-002, R-ACC-003, R-ACC-010.
- If you change it: e-mail delivery / OIDC are future options; keep invite tokens hashed.

### d-hashing
- Decision: Argon2id m=19 MiB t=2 p=1 on `spawn_blocking`, guarded by `App.hashing` semaphore (permits = CPU cores); unknown users verify a dummy hash.
- Why: 60 parallel logins would need >1 GB; equal timing hides which usernames exist.
- Serves: R-SEC-002, R-PERF-002, R-ACC-004.
- If you change it: all hashing must go through `auth::hash_password/verify_password`.

### d-shared-types
- Decision: REST API (Axum routes) + shared DTO crate `musicus-api`; no Dioxus server functions, no OpenAPI codegen.
- Why: stable API for future native app/scripts; isolates server from Dioxus 0.x churn; Range/caching need plain HTTP anyway; compile-time agreement.
- Serves: R-TECH-003, F-OFF-001 (future).
- If you change it: new endpoint ⇒ types in `dto.rs`, handler in server, helper call in web `api.rs`.

### d-pdf-rendering
- Decision: browser renders with pdf.js (legacy build, pinned 6.5.438, fetched by `scripts/fetch-pdfjs.sh`) through `js/bridge.js`; server uses hayro only for page count + thumbnail (best effort).
- Why: pdf.js = most compatible; hayro WASM adds 1.1–1.7 MB and is immature; modern pdf.js build needs `Map.prototype.getOrInsertComputed` (missing in tested Chromium and older tablets) → legacy build; hayro needs no C deps.
- Serves: R-READ-006, R-READ-001, R-FILE-003, R-TECH-002.
- If you change it: upgrading pdf.js ⇒ update VERSION+INTEGRITY in the script, keep legacy build, rerun e2e. Keep the renderer behind `bridge.rs` functions.

### d-no-client-storage
- Decision: client keeps sheets in a JS `Map` in memory only; server default `Cache-Control: private, no-store` (`MUSICUS_CLIENT_CACHE=immutable` opt-in); no localStorage/IndexedDB/Cache API/service worker.
- Why: brief requirement (nothing persistent on client until an encrypted app exists); prefetch still avoids network during the concert.
- Serves: R-SEC-001, R-FILE-007, R-PROG-003, R-PERF-007.
- If you change it: persistent storage only via the future encrypted app (F-OFF-001); conflicts recorded in requirements (R-PROG-003 ↔ R-SEC-001).

### d-ink-format
- Decision: annotations = vector objects (Stroke/Stamp/TextNote) in page-relative u16 coords (0..65535), widths in 1/65535 page width; encoding = version byte `1` + postcard; stroke points delta-coded (i32 deltas); server stores zstd(level 3) per page.
- Why: device/zoom independent; erasable per object; ~7 KB typical page vs ~61 KB JSON.
- Serves: R-ANN-005, R-ANN-007, R-ANN-006.
- If you change it: bump `FORMAT_VERSION`, keep decoding v1 (stored data!), update `decode` validation limits and the server's 4 MiB route limit consistently.

### d-optimistic
- Decision: per-page `revision`; PUT needs `If-Match: "<rev>"` (`"0"` = new page; missing ⇒ 428); mismatch ⇒ 409 `{current_revision}`; client fetches, `ink::merge(base_ids, local, server)`, retries (≤5). Objects immutable with random u64 ids.
- Why: one owner per set ⇒ conflicts only between own devices; immutable objects make id-based 3-way merge exact without locks/CRDT; revision is the future sync cursor.
- Serves: R-ANN-012, R-OFF-002 (future).
- If you change it: never mutate an object in place (e.g. moving a stroke = new id). Keep `base_ids` tracking in `viewer.rs::PageState`.

### d-204
- Decision: GET of a never-saved annotation page ⇒ 204 + `ETag: "0"` (not 404).
- Why: every page turn loads a layer; 404s polluted the console and were ambiguous with "no access".
- Serves: R-ANN-013.
- If you change it: client `api.rs::get_page` maps 204 → `PageBytes::Missing`; 404 now means no access / no set.

### d-reader-store
- Decision: reader annotation state in `Rc<RefCell<Store>>` (not signals); a `version` signal is bumped to redraw; pending saves flushed on unmount via `spawn_forever`; copies of dirty pages registered in `bridge.js` (`setPending`) and sent with `fetch(…, {keepalive:true})` on `pagehide`.
- Why: saves must outlive the component and the tab; signals of unmounted components must not be touched; avoids re-render per pen event.
- Serves: R-ANN-010, R-ANN-011.
- If you change it: every mutation of `PageState` must bump `generation` and call `sync_pending`; save completion must update `saved_generation`, `revision`, `base_ids` and call `sync_pending`.

### d-distroless
- Decision: runtime image `gcr.io/distroless/cc-debian12:nonroot` (UID 65532), no shell/curl; health check and backup are server subcommands.
- Why: small (~70 MB), minimal attack surface.
- Serves: R-SEC-007, R-OPS-007, R-OPS-003.
- If you change it: anything the container must do at runtime must be in the binary.

### a-workspace-members
- Decision: `musicus-web` is a workspace member but not in `default-members`.
- Why: it only compiles for wasm32; plain `cargo build/test/clippy` must work for server + api.
- Serves: R-TECH-002.
- If you change it: check web with `cargo clippy -p musicus-web --target wasm32-unknown-unknown`.

### a-ram-cache
- Decision: full-body GETs of files served from `FileCache` (LRU, `Bytes` shared); Range requests streamed from disk by `tower_http::ServeFile`.
- Why: burst of identical downloads served from one RAM copy.
- Serves: R-PERF-003, R-PERF-004, R-FILE-006.
- If you change it: cache key is the sha256 (immutable), so no invalidation logic exists or is needed.

### a-file-acl-any-part
- Decision: a file (by hash) is readable if the user may read ANY part that has ANY version pointing to that file.
- Why: identical uploads are deduplicated, so one blob can belong to several parts/versions; old versions stay readable for annotation sets.
- Serves: R-FILE-005, R-FILE-002, R-ANN-016.
- If you change it: keep `routes/files.rs::readable_file` consistent with `part_visible!()`.

### a-eager-prefetch
- Decision: `bridge::prefetch(url)` calls the JS function immediately and returns a future (`impl Future + use<>`); the download runs even before the future is polled. `programmes.rs` awaits chunks of 3 in order, which still downloads in parallel.
- Why: no extra futures/join dependency; parallelism comes from the browser.
- Serves: R-PROG-003.
- If you change it: if made lazy, the chunk loop becomes sequential.

### a-wasm-view-copy
- Decision: `bridge.js::setPending` copies `bytes.slice()`.
- Why: a Rust `&[u8]` arrives in JS as a view into WASM memory valid only during the call. A bug where pending strokes were lost on reload came from storing the view.
- Serves: R-ANN-011.
- If you change it: any JS function storing a `&[u8]` argument must copy it.

### a-live-ink
- Decision: while drawing, segments are painted imperatively on the ink canvas (`ink::segment`); the stroke is committed to the model on `pointerup`; once `pointerType == "pen"` was seen, touch is ignored.
- Why: 60–240 pointer events/s; re-rendering via VDOM would lag. Palm rejection.
- Serves: R-ANN-001, R-ANN-004.
- If you change it: keep the `Live` struct outside signals.

### a-unauthorized-reload
- Decision: any 401 from the API makes the client do a full page load of `/login` (except on `/login` and `/invite/*`).
- Why: discards all in-memory state of the previous session (sheets, annotations).
- Serves: R-SEC-001, R-ACC-008.
- If you change it: keep in-memory data from surviving a session change.
