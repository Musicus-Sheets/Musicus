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
- If you change it: adding a service (Redis, Postgres, S3) needs user consent; the PostgreSQL variant "A-PG" (PostgreSQL in compose) was evaluated and deferred by the user — measurements and the switch plan are in `background.md` §5; alternatives B–E and why they were rejected in §4. Keep `musicus-server` self-contained.

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
- Decision: grants are additive only (no deny rules) and always go to groups, never to single users. One SQL fragment `part_visible!()` = librarian flag OR instrument grant via group membership OR score grant via group membership. Embedded in every query that lists/loads parts, scores, programmes, files (via `part_files`), annotations (via `ensure_part_readable`). No permission cache. Denied ⇒ 404.
- Why: additive group grants keep access explainable ("you see it because you are in Trumpets and Trumpets has Trumpet"); ~25 µs per check at 200k parts; one definition so lists and point-loads agree; 404 hides library contents.
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

### d-inline-management
- Decision: no separate management area. Library, Programmes and Instruments are the same components for every role; they add controls (`+ Add piece`, `Edit` on pieces and parts, `+ New programme`, `Edit` on programmes, add/edit/delete instruments) when `ui::can_manage()`; editors live on their own routes (`/scores/new`, `/scores/:id/edit`, `/parts/:id/edit`, `/programmes/new`, `/programmes/:id/edit`). Groups (librarian/admin) and Users (admin) are their own tabs. `/manage…` redirects to `/`. Hiding controls is a convenience only: the server stays the authority for every role check (`require_librarian`/`require_admin`, `part_visible!()`); management pages show `ui::NoAccess` to other roles.
- Why: the owner wants librarians to work where the music is ("go to the piece, click Edit") instead of in a parallel "Manage" tab; members see exactly the same views without the controls, so documentation and support cover one UI.
- Serves: R-ACC-013, R-ACC-014, R-ACC-009, R-LIB-008, R-LIB-009.
- If you change it: never rely on hidden controls for security (R-ACC-009 is enforced server side; keep the 403 tests). A new management action belongs next to the content it changes, gated by `can_manage()`/`is_admin()`; update `e2e/smoke.mjs` (member/librarian control checks) and the user guide.

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

### a-submodule-pins
- Decision: the docs (requirements, technical docs, user guide) are separate repositories used as submodules with `branch = main` in `.gitmodules`. A main-repo pull request may only be merged when every submodule pointer is a commit on the first-parent history of the docs repo's `main` (`scripts/check-submodules.sh`, workflow `Submodules`, required check `pinned`). Order: merge the docs pull requests (merge commit or squash), then `./scripts/update-submodules.sh`, commit, push, then merge the main pull request.
- Why: the main repository must reference documentation that is actually on the docs `main`, and exactly the merge commits created there (the user's rule), not the feature-branch tips (the merge commits' second parents), which only exist as long as their branches do.
- Serves: no product requirement; documentation integrity (AGENTS.md maintenance rules).
- If you change it: keep the check a required status check; with a new submodule add `branch = main` to `.gitmodules`.

### a-assign-picker
- Decision: every "assign entries to X" list (group members, instrument and score grants, part instruments, programme pieces) is one component, `picker.rs`: assigned entries as removable chips + a dropdown with a focused search field (case/diacritics-insensitive, all useful text) and a multi-selection that applies when the dropdown closes (tap outside = transparent backdrop, Escape, `Add n` button); no cancel action (untick, or × afterwards). In editors of existing things (group editor, part editor) each change is saved immediately (full list `PUT`; controls disabled while it runs; error ⇒ message + previous list restored; "Saved" for 2.5 s); the group editor therefore has no Save button. The part editor's immediate instrument save sends the last *saved* label/voice/key/notes because `PUT /api/parts/{id}` replaces all fields; the other fields keep `Save part`. Creation forms (new part) and the programme editor (order + title are one unit, Save programme) only change form state. The programme picker does not exclude pieces already present (encores) and appends in tick order.
- Why: checkbox lists grow with every user/instrument/piece (owner's request); search + multi-tick avoids reopening per entry; apply-on-close matches the owner's mental model ("when I click elsewhere they are added"); Escape applying (not cancelling) makes every way of closing equal, so nothing ticked is lost by surprise; the backdrop swallows the outside click so a click on another group or a nav link cannot unmount the editor while the save starts. Immediate save avoids a forgotten Save button losing grants.
- Serves: R-ACC-015, R-ACC-016, R-ACC-017, R-ACL-004, R-LIB-009, R-PROG-001.
- If you change it: keep the server the authority (no new endpoints; members list only for admins). Making Escape cancel needs a visible cancel button and an R-ACC-016 change. Adding fields to `PartInput` ⇒ add them to `PartEditor::saved`. Update `e2e/smoke.mjs::pick` and the user guide.

### a-part-names
- Decision: a part's name is derived from its instruments, key and voice (`musicus_api::naming::part_name`: "Trumpet in B♭ 1", "Flute / Piccolo", "Part 2", "Unnamed part"); `parts.label` is an optional custom name that replaces it. The server computes `Part.display_name` on every read (it knows the instrument names), so renaming an instrument renames its parts; clients show only `display_name` and call `part_name` only for the live preview while editing. Keys come from the fixed list `naming::KEYS` stored as ASCII codes (Bb, Eb, F, C, A, D, G, E, Ab, Db, Gb, H) and shown with ♭/♮; B natural is "H" because a stored "B" is ambiguous (B♭ in German). The server normalises unambiguous spellings and rejects others with 400, except that a part may keep its stored legacy key (shown as "Other: …"). Voice is free text (≤20). Search over part names uses `part_fts`, written by the server (`part_index`) because the name depends on instrument names (no SQL trigger can compute it); each search word must match the score or a part the user may read. Migration 0002 keeps old labels as custom names (no visible change for existing data), converts numeric voices to text and maps only unambiguous key spellings.
- Why: the owner wants names to follow the metadata instead of a mandatory free-text label that duplicated (and could contradict) instrument, key and voice; a fixed key list keeps names and grants consistent; one function in the shared crate guarantees server and preview agree.
- Serves: R-LIB-002, R-LIB-004, R-LIB-010, R-LIB-011, R-LIB-012.
- If you change the format: change `naming::derived_name` and its tests, R-LIB-010, the user guide and `e2e/smoke.mjs` expectations; the index is rebuilt only when counts differ, so after a format change force a rebuild (e.g. `DELETE FROM part_fts` in a new migration). New keys: add to `KEYS` (and `parse_key` if they have spellings), R-LIB-011. Any new place that shows a part must use `display_name`; any new write to parts or instrument names must call `part_index::reindex` in its transaction.

### a-guide-site
- Decision: the user guide is published as a GitHub Pages site with one version per release, built by `scripts/build-guide-site.sh` and deployed by `.github/workflows/user-guide-pages.yml` with `actions/upload-pages-artifact` + `actions/deploy-pages` (no `gh-pages` branch). Every run rebuilds the whole site from scratch: each published, non-draft, non-prerelease release (`gh release list`) is built from the user-guide commit that its tag's `docs/user-guide` pointer names, using that commit's own `build.sh` and one pinned Typst (`TYPST_VERSION` in the workflow, the version the guide's README names). Layout: `/<tag>/` per release (HTML + PDF), `/latest/` = copy of the release with the highest version number (`sort -V` on tag names, not publication date, so a later patch of an old line does not become "latest"), `/` = version list, `/versions.json`. A manual run (`workflow_dispatch`, input `ref`, default the run's branch) adds `/test-<ref>/`, labelled "test build" in the list and in its version bar. A small version bar (version, PDF, "All versions" link) is injected right after `<body>` of each generated HTML; the guide sources are not changed. The version list ends with the license notice (guide CC BY-SA 4.0, Musicus AGPL-3.0, source link), the same text the Typst templates put on the PDF title page and at the end of each HTML page. Releases without `docs/user-guide` (older than the guide) or whose build fails are skipped with a warning; the run fails (nothing is deployed) if the requested test build fails or if no release with a user guide could be built. Prereleases do not trigger a run. No build cache: one version builds in about 1–2 s.
- Why: the deployed site is replaced as a whole on every deploy, so a release run (releases only) drops earlier test builds by construction, and a manual run keeps all releases without any state to clean up; the guide of a release is exactly the docs that were pinned in it (a-submodule-pins); a plain script can be run and tested locally without GitHub; copying `latest/` (instead of redirecting) keeps deep links with `#anchors` working.
- Serves: no product requirement; user documentation that matches the version an operator actually runs.
- If you change it: a release tag must still contain the user guide pointer; when bumping Typst check that old guides still build (they are rebuilt on every run; a failing one is skipped, not kept). The `github-pages` environment must allow deployments from release tags and from the branches manual runs use. Switching to a `gh-pages` branch (incremental deploys) would keep test builds forever unless a release run deletes them explicitly.

### a-tag-input
- Decision: tags are edited as atomic chips plus ONE ordinary `<input>` that is moved between the chips and acts as the caret (`tag_input.rs`); no `contenteditable`. The input moves with ←/→/Home/End only while it is empty; Backspace/Delete at its edges remove the neighbouring chip; clicking a chip places the input after it. Comma, Enter (never submits), Tab and blur finish a tag; commas are also read from the input event (Android, paste). Duplicates (case-insensitive) are refused with a flash of the existing chip. Data and API unchanged (`ScoreInput.tags: Vec<String>`; the server still dedupes and limits).
- Why: the owner wants tags as boxes like in the library with a caret between, but never inside, them. `contenteditable` with non-editable islands behaves differently in Chromium, Firefox and Safari (caret jumping into or over islands, IME composition, Android keyboards replacing whole words, undo) and would need its own model-to-DOM sync; a moved real input keeps native text editing, IME and mobile keyboards, and chips cannot be entered by construction. Blur commits so that a tag typed without a comma is not silently dropped when the form is submitted.
- Serves: R-LIB-001, R-LIB-013.
- If you change it: keep chips non-editable and Enter from submitting; keep the comma handling in the input event (mobile). If `contenteditable` is reconsidered, test Chromium, Firefox, Safari/iOS and an Android keyboard first. Update `e2e/smoke.mjs` (tag checks in the guided flow and the piece editor).

### a-tag-suggest
- Decision: suggestions while typing tags and instrument families are one client component (`suggest.rs`) instead of the browser `<datalist>` the family field used. Known tags come from a new endpoint `GET /api/tags` (no tag list existed; the library filter `?tag=` only filters), which returns the tags of the scores the user may see (librarians: all; `part_visible!()` for others), merged ignoring case. Ranking: starts with the text, then a word starts with it, then contains it; case and diacritics ignored; chosen tags excluded; at most 8. Nothing is preselected: ↓/↑ choose, Enter/Tab take the chosen one, Escape closes, click/tap takes (mousedown prevented so the input keeps focus).
- Why: `<datalist>` matching, ordering and keys differ between Chromium, Firefox and Safari, it cannot ignore diacritics or exclude chosen tags, and it does not fit the chip input (a taken value must become a chip). Tags are free text; members could otherwise learn tags of scores they cannot see, hence the ACL filter even though only librarians edit tags. Without preselection, Enter and comma keep finishing what was typed.
- Serves: R-LIB-014, R-LIB-013, R-ACL-001.
- If you change it: keep `list_tags` ACL-filtered (or restrict it to librarians); keep mousedown prevented on suggestions (otherwise the tag input commits the partial text on blur). Update `e2e/smoke.mjs` (family and tag suggestion checks).

### d-system-layout
- Decision: music systems are detected on the server, once per file (blobs are immutable), by a projection-based analysis of each page rendered with hayro (`layout::detect`), stored as JSON in `file_layouts` with `ALGORITHM_VERSION`; the client only plans views for its screen (`musicus_api::layout::plan_views`). Unsure pages (confidence < 0.6, no systems) are shown whole. Systems wider than the screen are shown at full width, never split (split at barlines = R-READ-017, proposed). Exact algorithm: `layout.md`, technical chapter "System view".
- Why: one analysis serves every member, never at the moment a piece is opened on stage; 23–112 ms/page on one core, no model, no new dependency (hayro already renders thumbnails); OMR would be far heavier and fail on manuscripts. Prototype: 82/84 music pages of 23 real scores correct (`background.md` §6.1).
- Serves: F-READ-003, R-READ-010, R-READ-011, R-READ-015, R-PERF-008, R-FILE-004.
- If you change it: any change of detection results ⇒ raise `ALGORITHM_VERSION` (all files are re-analysed in the background) and update `layout.md` + `docs/technical/chapters/12-system-view.typ` (constants are named the same in code and docs). Changing view rules ⇒ `musicus_api::layout` tests + R-READ-011/012/013.

### a-layout-worker
- Decision: one tokio task (`layout::worker::spawn`, started by `serve`) processes one file at a time on `spawn_blocking`, newest file first (`ORDER BY f.id DESC`); it sleeps on `App.layout_jobs` (`Notify`; uploads call `notify_one`, a permit is stored if the worker is busy). Failures are stored as `failed` (no retry until the version rises); DB errors back off 30 s. Tests call `run_once` directly (the test `Server` does not spawn the worker).
- Why: bounded CPU and memory on a Pi (one page raster at a time); a fresh upload is analysed before an old backfill; requests never wait; no job table needed because "no row or older algorithm" is the queue.
- Serves: R-READ-010, R-PERF-008, R-PERF-001.
- If you change it: more workers need a claim mechanism (two would pick the same file). Keep analysis off request paths.

### a-css-mm
- Decision: staff sizes are physical millimetres computed with a fixed `CSS_PX_PER_MM` = 132/25.4 (132 CSS px per inch), not the CSS `mm` unit (96 px per inch). S/M/L/XL = 5/6/7/8 mm, default M.
- Why: browsers do not report the physical density; tablets show 120–160 CSS px per inch (iPad 132), so CSS mm would be about 30 % too small. Sizes from the prototype data: an A4 part on an 8" tablet in landscape reaches 5–7.5 mm at full width.
- Serves: R-READ-014, R-READ-011.
- If you change it: a calibration setting per device would need client storage (R-SEC-001) or a server setting per user and device.

### a-footer-attach
- Decision: ink that starts within `FOOTER_ATTACH_SPACES` (4 line distances; 2.5 in the first version, which turned a dynamic mark 2.4–3 spaces below the last staff into a skipped footer) below the last staff of a page belongs to the last system (up to the next ink-free run of one line distance), even if the margin scoring chose a cut above it.
- Why: the prototype's only unflagged error (lake-pirates p. 2): an 8vb line and a low note became footer, and small footers are skipped by the reader, so music was lost. A copyright line that close is attached too — harmless (it is only shown).
- Serves: R-READ-010, R-READ-013.
- If you change it: raise `ALGORITHM_VERSION`; keep the unit test `ink_right_below_the_last_staff_stays_with_it`.

### a-view-anchor
- Decision: the reader stores the reading position in the system view as the index of the first item of the current view (`anchor`), not the view index; the shown view is `view_for_item` = the last view starting at or before the anchor. Views are re-planned from scratch on every size/setting change.
- Why: view indices change when the screen turns or the staff size changes; the anchor keeps the music being read on screen. With the one-system overlap an item can end one view and start the next; "last view starting at or before" picks the one the musician turned to.
- Serves: R-READ-011, R-READ-012.
- If you change it: keep `position_survives_a_new_plan`.

### a-pdfjs-wasm
- Decision: `fetch-pdfjs.sh` also copies pdf.js' `wasm/` decoders (JBIG2, OpenJPEG, qcms incl. their non-wasm fallbacks, licences; not QuickJS) and `bridge.js` passes `wasmUrl: '/vendor/pdfjs/wasm/'` to `getDocument`.
- Why: without them pdf.js 6 renders pages of JBIG2/JPEG 2000 scans (common on IMSLP) blank — found while testing the system view on real scans; affected the page view too.
- Serves: R-READ-006, R-READ-001.
- If you change it: a pdf.js upgrade must keep `wasmUrl` and the copied file names.

### a-layout-corrections
- Decision: librarians' corrections are stored per (file, page) in their own table `layout_corrections` as the complete band list (`PageCorrection`: cut lines + kinds + whole-page flag), not as edits of the detected regions, and applied when the layout is served. One correction per page, shared by all members (the file is the same for everyone); `PUT`/`DELETE` only for librarians; the editor is a normal Shell page reached from `Edit part` (d-inline-management).
- Why: re-detection (raised `ALGORITHM_VERSION`) rewrites `file_layouts` and must not lose manual work; a complete band list stays valid whatever the new detection finds; staff sizes still come from the detection (librarians only move lines). A corrected file version is a new file ⇒ no stale corrections.
- Serves: R-READ-016, R-READ-010, R-ACL-001.
- If you change it: keep corrections out of the worker; a new file version starts uncorrected (tell librarians in the guide).

### a-contiguous-view
- Decision: the detected systems only decide where a system view starts and ends. On each page a view shows ONE contiguous crop of the original PDF page, from the top of its first item to the bottom of its last, with everything between (gaps, text, bands marked hidden or small headers/footers that lie between shown systems); hidden/skipped bands matter only at the start or end of a view. One horizontal range and one scale per document (from the minimum staff size or the screen width, whichever is smaller); no stitching of separate system crops, no removed gaps, no shadows or borders between systems; a view across a page boundary stacks two crops directly, with a 1 px hairline.
- Why: user feedback on the first version (one crop per system, each with a shadow, gaps removed, per-view x-range and scale that filled the screen): a line drawn over several systems jumped at every system boundary, the patchwork did not look like the original sheet, and views changed size/position. A contiguous crop keeps strokes straight and 1:1 with page coordinates and keeps the familiar page image. Price: the space between systems is shown, so slightly fewer systems fit; the music does not grow to fill a screen that has room left (it stays at the chosen minimum size or full width).
- Serves: R-READ-011, R-READ-013, R-READ-018, R-ANN-005.
- If you change it: keep `a_view_is_one_contiguous_crop_per_page_with_one_x_range` and the smoke checks (one canvas per page segment, no shadows, straight stroke across systems).
