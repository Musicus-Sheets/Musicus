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
- Decision: a part's name is derived from its instruments, key and voices (`musicus_api::naming::part_name`: "Trumpet in B♭ 1", "Klarinette 1 & 2" (a-part-voices), "Flute / Piccolo", "Part 2", "Unnamed part"); `parts.label` is an optional custom name that replaces it. The server sends the components (`Part.instruments` with names as entered); the client composes the name in its language on every render (a-client-part-names), so renaming an instrument renames its parts. Keys come from the fixed list `naming::KEYS` stored as ASCII codes (Bb, Eb, F, C, A, D, G, E, Ab, Db, Gb, H) and shown with ♭/♮; B natural is "H" because a stored "B" is ambiguous (B♭ in German). The server normalises unambiguous spellings and rejects others with 400, except that a part may keep its stored legacy key (shown as "Other: …"). Voices are a list of free texts (a-part-voices). Search over parts uses `part_fts`, written by the server (`part_index`; components and key spellings) because it depends on instrument names (no SQL trigger can compute it); each search word must match the score or a part the user may read. Migration 0002 keeps old labels as custom names (no visible change for existing data), converts numeric voices to text and maps only unambiguous key spellings.
- Why: the owner wants names to follow the metadata instead of a mandatory free-text label that duplicated (and could contradict) instrument, key and voice; a fixed key list keeps names and grants consistent; one function in the shared crate guarantees server and preview agree.
- Serves: R-LIB-002, R-LIB-004, R-LIB-010, R-LIB-011, R-LIB-012.
- If you change the format: change `naming::derived_name` / the `part-name-*` messages and their tests, R-LIB-010, the user guide and `e2e/smoke.mjs` expectations; the index is rebuilt only when counts differ, so after a format change force a rebuild (e.g. `DELETE FROM part_fts` in a new migration). New keys: add to `KEYS` (and `parse_key` if they have spellings), R-LIB-011. Any new place that shows a part must use `names::part_name` (client); any new write to parts or instrument names must call `part_index::reindex` in its transaction.

### a-part-voices
- Decision: a part has an ordered list of voices (`Part.voices`/`PartInput.voices: Vec<String>`) instead of one optional voice, stored relationally in `part_voices (part_id, position, voice)` (WITHOUT ROWID, PK (part_id, position), UNIQUE (part_id, voice), `voice` COLLATE NOCASE, index on `voice`), not as a JSON column. The derived name joins the voices with `naming::VOICE_SEPARATOR` " & " in stored order: "Klarinette 1 & 2", "Flöte / Oboe 1 & 2", three or more also with " & " ("Trombone 1 & 2 & 3"; the owner's choice, chosen over "1, 2 & 3" because "," inside a name reads like a list of parts and one separator keeps the name ↔ voices mapping trivial). Input is cleaned by `naming::clean_voices` (shared by DTO, server and client): entries split at "&", trimmed, blanks and duplicates ignoring case dropped, order kept; the server limits 10 voices of ≤20 chars (400). Sorting is by the shown name with the browser collator (numeric, a-browser-sort), which orders by the first voice, then the rest. Search needs no change: "1 & 2" tokenises to `1`, `2` in `part_fts`, so "Klarinette 2" finds "Klarinette 1 & 2". The web client edits voices with the tag chip input (`TagInput`, noun "voice", separators ",&"). The old single `voice` field of the API was removed without a compatibility alias (nothing released; serde ignores the unknown field). Migration 0005 copies each old voice unsplit as the only entry and empties `part_fts` for a rebuild.
- Why: the owner plans grants per voice ("2nd clarinets only", R-ACL-005, proposed): Musicus must know which voices a part contains, and a relational table lets a later `part_visible!()` extension join `part_voices` by index exactly like `part_instruments`; a JSON column could only be scanned with `json_each`. `part_visible!()` is unchanged for now. "&" is a separator, not voice text, so a name never hides how many voices a part has (and the importer can parse names back).
- Serves: R-LIB-002, R-LIB-004, R-LIB-010, R-ACL-005 (prepares).
- If you change it: the separator lives in `naming::VOICE_SEPARATOR` (split char), the message `part-name-voice-sep`, the client `separators` prop, R-LIB-010, the user guide and `e2e/smoke.mjs`; a format change needs a `part_fts` rebuild (new migration `DELETE FROM part_fts`). When grants per voice come: join `part_voices` in `part_visible!()` (keep the NOCASE collation consistent with `clean_voices`), keep the voice index.

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

### a-device-pixels
- Decision: the reader chooses every canvas size in whole device pixels and derives the CSS size from them
  (`musicus_api::pixels::device_size`, mirrored by `bridge.js::deviceSize`): pixels = round(CSS × ratio), CSS = pixels /
  ratio; pdf.js renders with viewport scale = pixels / width in pt (no extra transform); the visible canvas gets the bitmap
  1:1. Ratio = `devicePixelRatio` × pinch zoom (quarter steps), watched (`pixelRatioChanged`: resolution media query,
  resize, visualViewport) and re-rendered on change; capped at `MAX_CANVAS_PIXELS` (16.7 Mpx, Safari's limit) per canvas.
  Ink canvases use the same pixels (`ink::prepare(…, ratio)`), so ink and PDF stay aligned. Crop bitmaps are cached by
  pixel size. No sub-pixel position snapping (no CSS `translate`).
- Why: "sometimes the PDF is blurry". Measured on real high-density Chromium (`--force-device-scale-factor`, compare a
  screenshot with the canvas pixels): browsers place a canvas on whole device pixels themselves and paint it
  round(left + width) − round(left) pixels wide; the old code took a fractional CSS size (fit scale) and floor/round of
  CSS × DPR, so the painted size differed by one pixel (page 1045 backing vs 1046 painted at DPR 2, crop 1299 vs 1300,
  2117 vs 2118 at 2.625) and the whole bitmap was resampled (mean abs. difference 4–6 grey levels, 60 % more grey pixels
  = soft staff lines); it depended on the page size and zoom, hence "sometimes". A DPR change without resize (other
  screen) kept the old bitmap stretched 2×, a pinch zoom magnified the bitmap. After: painted = backing in all cases,
  difference 0. A translate onto the pixel grid was tried and dropped: unnecessary on real screens, and DSF emulation
  (Playwright `deviceScaleFactor`) lays out in CSS pixels and scales afterwards, so it cannot show 1:1 at all — sharpness
  tests need a real device scale factor.
- Serves: R-READ-001, R-READ-011, R-ANN-005.
- If you change it: keep CSS size = pixels / ratio for PDF and ink canvases (and `.page-segment` boxes), keep
  `pixels.rs` tests and the smoke checks "painted 1:1, not resampled" (Chromium with `--force-device-scale-factor=2`).

### a-archive-number
- Decision: `scores.archive_number` is optional free text (≤ 40 chars, trimmed, blank = none), NOT unique and without format rules. The library search indexes it through a hidden column `scores.archive_search` = `naming::archive_search_text` (the number as written + letters/digits only + without leading zeros: "0304" ⇒ "0304 304", "A-12" ⇒ "A-12 A12"), computed by the server on every score insert/update (`library::insert_score`, `update_score`) and indexed by `score_fts` like the other columns. Matching (import) compares `naming::archive_key` (letters/digits, lower case, leading zeros of a leading number dropped). Shown as a badge "No. …" in front of the title for everyone who sees the piece; the library offers "Sort by archive number" (client-side, `locale::compare` with numbers by value, pieces without number last; choice kept in memory only).
- Why: real paper archives have gaps, double numbers, typos and letter prefixes; a unique constraint would block importing such an archive and gives no safety (the import only suggests a match, the librarian confirms). The search forms let librarians type the number as they think of it ("304" for folder "0304"); computing them in Rust keeps SQLite free of string gymnastics and lets FTS5 `'rebuild'` work (the column exists in `scores`). Members may see the number: it is printed on the paper folder they get anyway.
- Serves: R-LIB-015, R-LIB-004, R-LIB-001.
- If you change it: every write of `scores.archive_number` must also write `archive_search` (use `insert_score`; `update_score` does both). A changed search form needs a new migration that recomputes `archive_search` (it is server-computed, so SQL alone cannot — e.g. a start-up rebuild like `part_index`). Making the number unique needs R-LIB-015 changed and a plan for existing duplicates.

### a-import-shared
- Decision: the whole interpretation of an import (path patterns, part-name parser, dictionary, grouping, resolution against instruments/aliases/pieces/groups, colours) is pure code in `musicus_api::import`, used by the server for the stored proposals, the review and the commit check, and by the web client for the live preview while a pattern is chosen. The server stores only the parse result (`PieceGuess`/`PartGuess`, candidates with `Source`) and the librarian's overrides; resolution runs on every read against the current library, so a new instrument or alias changes all rows at once without rewriting them. Every value carries a `Source`; `inventory` and `ocr` are reserved for phase 2 (R-LIB-022), which adds candidates without changing the workflow.
- Why: the preview and the server must agree exactly (the user's requirement); keeping resolution out of the stored data avoids stale colours and an "apply alias to all rows" write over thousands of rows.
- Serves: R-LIB-017, R-LIB-018, R-LIB-019, R-LIB-020, R-LIB-022, R-TECH-003.
- If you change it: parsing changes alter proposals only for new batches or after "re-read" (`PUT` batch, see server docs); `Note`s are data, their text is rendered by the client (`musicus-web/src/errors.rs::note_text`, a-error-codes) — the server never turns a note into a sentence; new sources add `Source` variants and candidates (the first candidate wins unless overridden, more than one distinct value is red).

### a-import-patterns
- Decision: own mini pattern language instead of regular expressions: placeholders `{number}` `{title}` `{composer}` `{arranger}` `{genre}` `{part}` `{*}`, optional groups `[…]`, everything else literal. Optional groups are expanded into alternatives, tried with the most groups present first and, among equally many, the earlier groups of the pattern first; matching is backtracking per alternative with `{number}` and `{*}` as short as possible and text fields as long as possible, never across `/`. `{number}` needs a digit and no spaces, so "Alte Kameraden" is never taken as number while "A-12" is.
- Separators are tolerant instead of one preset per separator: a space in the pattern also matches `_` (as `_` already counted as a space in values), and values are trimmed of `-`, `–` and spaces at both ends. So `{number} {part}` reads `0304 Horn`, `0304_Horn`, `0304 - Horn` and `0304_-_Horn`, and `[{number} ]{title}` reads `0304 Title` and `0304 - Title`. A literal `_` or `-` in a pattern still matches only itself.
- Presets describe general archive layouts, never one ensemble's naming habits (the user's rule; an earlier preset copied one archive's file names and was removed): `folder` `[{genre}/][{number} ]{title}[ - {composer}]/{part}.pdf` (default; the optional category level replaces the former separate "category" preset), `flat-number` `{number} {part}.pdf`, `flat-title` `[{number} ]{title} - {part}.pdf`. Anything special (a counter like `Part06`, another order) is the librarian's own pattern (user guide example `{number}Part{*}_{part}.pdf`).
- Detection (`detect`): most matched paths, then most part names with a dictionary instrument, then most part names understood completely (no leftover words), then preset order (stable sort, deterministic). The third key separates `0304 Title - Horn` (flat-title: part "Horn"; flat-number would leave the title in the part); the order puts flat-number before flat-title, so `0304 - Horn` gives number 0304, not title "0304".
- Why: librarians must be able to read and edit the pattern; regexes are unreadable for them and allow catastrophic backtracking. Greedy titles make the last " - " the composer/part separator ("Marsch - Teil 2 - Fučík"); lazy ignored text keeps underscores of the part name in the part. Separator tolerance keeps the preset list short (three) and still covers the usual ways of writing number and part; one preset per separator would multiply presets and ties.
- Serves: R-LIB-017.
- If you change it: keep `PRESETS` general and matching their `example` (test `errors`); keep the tie tests (`detection_ties_are_decided_sensibly_and_deterministically`); per-subfolder patterns (phase 2) should reuse `Pattern` per folder prefix.

### a-import-keys
- Decision: the import has its own key reader `parts::parse_import_key(text, language)`; `naming::parse_key` (piece form, API input, migration 0002) is unchanged. A key is never assumed (no key word ⇒ no key, e.g. "Klarinette 1" has none). Unambiguous spellings (Bb, B♭, Eb, F, C, …, "b flat") work in both languages; German letters B (=B♭), Es, As, Des, Ges, H (=B♮) only in batches whose naming language is German (batch setting, suggested from German/English instrument words by `suggest_language`). In an English batch such a letter is yellow `AmbiguousKey` with no value; two different keys in one name are red.
- Why: the user's rule "key optional, never assume one"; "B" means B♭ in German and B♮ in English, the same reason `parse_key("B")` returns None (a-part-names). Changing `parse_key` would silently change the API behaviour.
- Serves: R-LIB-018, R-LIB-011.
- If you change it: keep tests `no_key_is_ever_assumed`, `german_letters_only_in_german_batches`.

### a-import-voices
- Decision: the part-name parser returns the voices as a list (`parts::Voices {values, level, note}`, a-part-voices), in the order written, without duplicates. An explicit separator directly between two voices makes the list sure (green): `+`, `&`, `/`, `,` (the tokenizer turns all four into the word `+`), `und`, `and` — "Trompete 1+2", "1/2", "1 & 2", "I+II", "1 und 2" ⇒ ["1", "2"]. Voices that only stand next to each other ("Trompete 1 2", or with other words between: "Klarinette 1 in B 2") are still read as a list but yellow (`Note::SeveralVoices`) for the librarian to check. The review shows the voices as chips in the same `TagInput` as the part editor (comma, "&" or Enter finish a voice, × removes one, at most `naming::MAX_VOICE_CHARS` per entry); every change sends the whole list as `PartOverrides.voices`, which replaces the list (cleaned with `naming::clean_voices`, ≤ `naming::MAX_VOICES`); the commit fills `PartInput.voices`.
- Why: with a voice list there is nothing left to decide for explicit separators (before, "1+2" was a single voice text and every multi-voice name needed checking). Loose numbers stay a real ambiguity: archive file names carry copy numbers ("Trompete 1 2" = second copy of the 1st trumpet), page or edition numbers, and a wrong voice would later give wrong access per voice (R-ACL-005). "/" between instruments ("Flöte/Piccolo") is harmless: a separator only joins voices when a voice stands on both sides.
- Serves: R-LIB-018, R-LIB-019, R-LIB-002.
- If you change it: keep test `several_voices` (parts.rs); the separator list lives in `tokenize` and `parse_part`.

### a-import-instruments
- Decision: in the import review the librarian sets any number of instruments per part, with the picker of the part editor (`picker::Picker`, a-assign-picker) and a × per instrument chip. The override (`PartOverrides.instruments`) is the whole list as `InstrumentPick`s: an instrument chosen from the library by id (green, source librarian), a detected instrument by its file-name word (`Word(alias)`), which keeps resolving like the detection (alias, library name, dictionary; colour and note as detected). The web client builds the list from what is shown (`ReviewPart::instrument_picks`) and adds or removes one entry.
- Why: adding an instrument to a part whose name contained a still unknown word must not drop that word silently — it stays red and in "Instruments to confirm" (Musicus never creates instruments by itself), and mapping it later via an alias still applies. A list of ids only would have lost it.
- An instrument is listed once per part (the user's rule), at every stage: the parser keeps one `InstrumentText` per dictionary entry (or unknown word), at its first place with the surest spelling ("Klarinette Klar 1" ⇒ Klarinette, green); the review keeps one `ResolvedInstrument` per resolved library instrument id (two words or an alias and a chosen instrument resolving to the same instrument) or per unknown word (`norm(alias)`), the surer at the earlier place (`push_unique`); `PartOverrides::merge` normalises words with `alias_key` and drops repeated picks. The name, the readers, "Instruments to confirm" and the commit are built from that list, so adding an instrument the part already has is a no-op.
- Serves: R-LIB-019, R-LIB-018, R-LIB-002.
- If you change it: keep the untagged JSON (numbers = ids, strings = words) and the `instrument_ids` alias for stored overrides; the server validates ids (`json_each`) and the count (≤ 20); keep tests `several_instruments_and_voices_chosen_in_the_review`, `an_instrument_detected_twice_is_listed_once` (review.rs) and `an_instrument_named_twice_is_listed_once` (parts.rs).

### a-import-dictionary
- Decision: a built-in dictionary of ~45 instruments (wind band first, orchestra basics) with German and English full words and abbreviations, folded (lower case, umlauts/diacritics removed). Full words are green, abbreviations and guesses (word start of a unique entry, or a longer form like "Klarinetten") yellow, "Bass" alone is ambiguous (yellow). "Partitur" (Partitur, Score, Full score) and "Direktion" (Direktion, Direction, Dirigierstimme, Conductor, Conductor score, Condensed score) are separate entries and never map to each other or to another instrument; "Conductor score" is one match (longest match wins) and means Direktion. The dictionary only proposes: the library instrument is found by stored alias, exact name, or a dictionary spelling of the library instrument's name; otherwise the word is unknown (red) and the librarian maps it (R-LIB-020). Filler words (`FILLER`: "Stimme"/"Stimmen", "part", "voice", ordinal endings, articles) are general German/English vocabulary for "part", not one archive's habit, and are skipped wherever they stand ("1. Stimme Horn", "Horn part 2").
- Why: the user's decision (Partitur and Direktion are two new instruments; rare pieces have both); a condensed score is a conductor's part, not a full score. Never creating instruments keeps R-LIB-003/R-ACL-001.
- Serves: R-LIB-018, R-LIB-020, R-LIB-003, R-ACL-001.
- If you change it: adding words is safe; never add an abbreviation that is also a key word (e.g. "db", "as") — dictionary lookup comes before keys.

### d-import-staging
- Decision: a mass import never writes into the library directly. Files go into a staging area: `import_files` rows (browser-computed SHA-256, state) with ordinary blobs and `files` rows but no `part_files`, so `files::readable_file` (and therefore `/api/files/{sha}`, thumbnails, layouts) never serves them to anyone; the review reaches them only through librarian-only batch routes (`/api/import/files/{id}[/thumbnail]`, `staged_blob`, which reads `files` through `import_files`, not through parts — the one place a file is read without `part_visible!()`, guarded by `require_librarian`). Proposals (`import_items`) are recomputed from the paths whenever the file list, pattern or language changes (`repropose`; the librarian's overrides survive by piece grouping key and part path). Duplicates: content already referenced by `part_files`, or the same content under another path of the batch (the first path in byte order is kept: `register_files` sorts), is `duplicate` and never imported; a blob that exists without being in the library (earlier batch) needs no upload. Resume = register the same folder again; only `pending`/`failed` files are requested. Commit = one write transaction per piece through the same helpers as the forms (`library::clean_score`/`insert_score`/`clean_part`/`insert_part`/`add_version`, `part_index::reindex`); red pieces are refused, "commit all" takes green pieces only. Undo = one transaction deleting the batch's parts and the pieces it created, refused as a whole (409, message names the pieces) if any imported part has annotation sets, a created piece got other parts, or is in a programme; blobs stay. Instruments are never created by the import (R-LIB-003); `instrument_aliases` maps file-name words chosen by a librarian.
- Why: the instrument decides who reads a part (R-ACL-001); staging + explicit review keeps wrong guesses away from members, and the absence of `part_files` makes that true by construction rather than by an extra flag every query must check. Short per-piece transactions keep annotation saves flowing during big imports (d-sqlite). Refusing undo as a whole keeps it predictable and never destroys musicians' work (annotations cascade with parts).
- Serves: R-LIB-016, R-LIB-019, R-LIB-020, R-LIB-021, R-ACL-001, R-ACL-002, R-FILE-004, R-PERF-006, R-LIB-003.
- If you change it: never add `part_files` for staged files before the commit; any new read of a staged file must go through its batch and `require_librarian`. Keep commit and undo per piece / per batch in `app.db.write` transactions. A ZIP upload (phase 2) should unpack in the browser or into the same `import_files` rows.

### a-import-page
- Decision: the mass import has its own navigation tab `Import` (librarians/admins) with `/import` (batches + new import) and `/import/:id` (review), plus an `Import folder` link next to `+ Add piece` in the library — an exception to d-inline-management like Groups and Users.
- Why: an import concerns hundreds of pieces that do not exist yet, so there is no piece to put an `Edit` next to; the review is a work list that librarians return to over days (resume, undo). The library link keeps the entry point where librarians add music.
- Serves: R-LIB-016, R-LIB-019, R-ACC-013, R-ACC-009.
- If you change it: keep the server checks (`require_librarian` on every `/api/import` route; members get 403); update `e2e/smoke.mjs` (nav checks) and the user guide.

### a-import-hashing
- Decision: the browser computes SHA-256 of every selected file before uploading anything: Rust `sha2` in WASM over 4 MiB `Blob.slice` chunks (`bridge::hash_import_file`); SubtleCrypto is not used because it cannot hash incrementally (a 200 MB scan would have to be read into memory at once). Uploads send the `File` object itself as fetch body, so the browser streams it from disk. Selected files live only in a JS array of the page (no IndexedDB/OPFS); resuming means selecting the folder again. `sha2` is compiled with opt-level 3 also in debug builds.
- Why: the server must know before any upload which files are duplicates or already uploaded (R-LIB-016: never twice, safe resume); hashing is the only way to recognise renamed copies. Nothing may be stored on the device (R-SEC-001).
- Serves: R-LIB-016, R-SEC-001, R-FILE-004.
- If you change it: the server checks the announced hash on upload (422 on mismatch), so a different hash algorithm needs both sides. ZIP upload (phase 2) would need unpacking in the browser to keep per-file hashes.

### a-error-codes
- Decision: the server sends no text for users. Every error body is `ApiError {error, code, args, message}`: `error` the generic class of the HTTP status (as before), `code` + `args` a `musicus_api::ErrorCode` (59 stable snake_case codes with typed arguments, serde adjacently tagged and flattened), `message` the code's English `Display` for API users, logs and `curl` only. `AppError` variants take an `ErrorCode` (no `String` constructor except `Internal`, which is only logged), `ApiError::new`/`ImportItemError::new` derive the message from the code, and `tests/no_prose.rs` rejects hand-built bodies. Shared data that used to carry English becomes data too: `Note` (no `message()` any more), `PatternError` (serde, `Twice(String)`), `InkError` (`Malformed` without the postcard text, `DuplicateId`, `PressureMismatch`, `LimitExceeded(InkLimit)`), `PageCorrection::validate` returns a code, `ImportItemError` carries a code, and `import_files.problem` stores the JSON of a code (migration 0008). The web client renders all of it in one module (`musicus-web/src/errors.rs`) with exhaustive matches, so a new code does not compile without a text; each arm is one catalogue message (`locales/en/errors.ftl`: `error-<code>`, `error-field-<field>`, `note-<kind>`, `error-pattern-*`, `error-ink-*`; d-i18n-fluent). Field names are a `Field` enum, not text. User data inside arguments (a path, a title, a voice) is passed as entered.
- Why: only the client knows the user's language (F-I18N-001); codes also serve a future app with its own texts. Typed args (instead of a loose map) keep server and client in sync at compile time. Keeping an English `message` makes the API readable without the client and lets a client show something for a code it does not know (`#[serde(other)] Unknown`, newer server); the client parses `{error, message}` loosely when a newer code has arguments.
- Serves: R-I18N-004, F-I18N-001, R-SEC-008, G-008.
- If you change it: a new error = a new `ErrorCode` variant (+ `Display`, `examples()`, the test index in `error.rs`, an arm in `errors.rs` with a new `error-<code>` message); never rename or reuse a code (changed meaning = new code), and never put a sentence in `args`. API tests assert codes (`Reply::error`), not texts. Changing the wire shape breaks every client.

## Internationalisation (F-I18N-001)

Concept and plan: `docs/technical/chapters/13-i18n.typ`. Terms and agreed translations: `glossary.md`.
Implementation state of each decision: see its **State** line (and `status.md`).

### d-i18n-fluent
- Decision: user-interface texts are Fluent messages (`.ftl`, Project Fluent) in a new crate `crates/musicus-i18n` (`locales/<lang>/<area>.ftl`, English = source of truth). A build script turns the English catalogue into typed functions (`t::library_add_piece()`, `t::library_parts_without_pdf(count)`), so a missing message or a wrong argument is a compile error; all shipped languages are compiled into the client (`include_str!`), formatted by `fluent-bundle` at run time with English as fallback per message. Dates, numbers and sorting come from the browser's `Intl` (a-intl-js, a-browser-sort), not from Rust libraries.
- Why: measured in a wasm32 benchmark (opt-level z, LTO): `fluent-bundle` + plural rules ≈ +60 KB gzip (≈ +8 % of the client), while `icu_datetime` would add ≈ 217 KB and `icu_collator` ≈ 418 KB gzip. Fluent has CLDR plurals and selectors (translators control word order, e.g. "Clarinette en si♭"), one message per id, translator comments, and is supported by Weblate, Pontoon and Crowdin. gettext/PO has no selectors and no compile-time check without own code generation (and `gettext-rs` needs libintl, not WASM); `rust-i18n` has weak plurals; ICU4X MessageFormat is not usable. Own code generation instead of `i18n-embed-fl` (`fl!` macro): typed functions instead of a checked macro, no `rust-embed`/`i18n-embed` loader machinery and proc-macro in the WASM build, arguments checked as function parameters.
- Serves: F-I18N-001, R-I18N-003, R-I18N-005, R-I18N-010, G-008.
- State: implemented (phase 1): crate `musicus-i18n`; measured client (dx release, wasm-opt, gzip -9): 2 233 709 B / 768 027 B gzip before, 2 353 949 B / 820 992 B after (+120 KB raw, +53 KB gzip). Phase 6 ships German (`locales/de`, a-i18n-german): release client 2 626 077 B / 885 735 B gzip without and 2 661 965 B / 896 671 B gzip with German (+35.9 KB raw, +10.9 KB gzip; the `.ftl` sources are embedded as text). `i18n-embed-fl` not built (not needed: own generator already gives typed functions).
- If you change it: the client and the search tables (`KEY_SPELLINGS`, `NAME_CONNECTORS`) depend on the catalogue format; a different format needs a new code generator and the CI checks (unused keys, placeholder parity). Keep dates/numbers/sorting in `Intl` unless the size budget changes (measure first).

### a-i18n-keys
- Decision: message ids are `<area>-<page or component>-<what>` in lower kebab case (`library-score-edit`, `reader-sets-new`, `common-save`); the generated Rust function is the id with `_` (`t::library_score_edit()`). An id is never reused for another meaning: a changed meaning gets a new id. One message per visible string (no attributes), arguments named in the message (`{ $count }`); counts are numbers so plural selectors work. Comments above a message (`# …`) tell translators where it appears. Files per area: `common`, `names`, `library`, `reader`, `import`, `admin`, `account`, `programmes`, `errors` (ids `error-<code>`, `error-field-<field>`, `note-<kind>`, `error-pattern-*`, `error-ink-*`). Text that is user data never enters the catalogue.
- Why: stable, greppable ids; translation tools key on ids, so reusing one would silently keep an old translation of another meaning.
- Serves: R-I18N-003, R-I18N-010.
- State: applied (phases 1–4): about 560 English messages in 9 files (with `errors`); the client has no literal UI text, error texts included (lint `scripts/check-ui-literals.py` without exceptions).
- If you change it: renaming an id loses its translations in every language (and in Weblate); prefer adding a new id and deleting the old one.

### a-i18n-german
- Decision: German ships as the second language (`crates/musicus-i18n/locales/de`, every English message translated) although it is an agent-written draft; it is marked "draft, needs maintainer review" in `glossary.md` and `status.md`, and the review aid `scripts/i18n-review.py` prints key | note | English | German. Form of address "du" (draft decision, glossary § Form of address). The German catalogue must stay complete: `tests/catalogue.rs` fails when an English message has no German counterpart (`COMPLETE`), when arguments differ, when a message sits in another file than its English source, or when a plural variant is not a German CLDR category (`one`, `other`) or string-selector key of the English message. German may add plural selectors where English has a fixed form. Key names B/Es/As/Des/Ges/H are glossary rules (agreed) and become search spellings; migration 0010 empties `part_fts` so existing indexes get them (a-client-part-names).
- Why: the user's decision to ship German now (browsers preferring German get German, others English, R-I18N-001, R-I18N-002); an incomplete German catalogue would silently mix English into German pages (R-I18N-003 makes it legal, so a test must forbid it). "du" fits an amateur ensemble's own tool; the maintainer can switch to "Sie" in one pass.
- Serves: R-I18N-001, R-I18N-002, R-I18N-003, R-I18N-005, R-I18N-007, F-I18N-001.
- State: implemented (phase 6); review by the maintainer open (glossary § Open choices). User guide and screenshots stay English (phase 7); `screenshots.mjs` and `smoke.mjs` force `locale: en-US`.
- If you change it: a new English message needs its German draft in the same commit (the test fails otherwise); tell the maintainer which German texts are new. Removing `de` from `COMPLETE` would let English leak into German pages.

### a-data-as-entered
- Decision: the database stores exactly what users type (titles, composers, tags, notes, instrument names and families, group names, voices, custom part names, programme texts, set names typed by users) and the client shows it unchanged in every language; nothing is ever translated or reformatted. The server produces no user-visible prose: part names, key names and the name of an unnamed annotation set are composed by the client in its UI language (a-client-part-names); errors are codes (a-error-codes). There is no instance-wide naming language, no key notation setting and no translated instrument proposal.
- Why: the user's decision; the data belongs to the ensemble ("Klarinette" stays "Klarinette" for an English reader), and only the words Musicus adds follow the reader's language.
- Serves: R-I18N-006, F-I18N-001.
- State: decided (phase 0); part names and set names client-side from phase 3.
- If you change it: translating data would need per-language columns and a source language per entry — ask the user.

### a-client-part-names
- Decision: the `Part` DTO carries the components of a name, not the name: custom `label`, `instruments: Vec<NamedRef {id, name}>` (names as entered; `instrument_ids` kept), key code, voices. `musicus_api::naming::part_name(words: &NameWords, …)` keeps the pure assembly (alphabetical instruments, dedup, voice cleaning, custom name wins) and takes the language-dependent words (`NameWords`: "{instruments} in {key}", "Part", "Unnamed part", " / ", " & ", key names) as a parameter; the client fills it from the catalogue and sorts parts itself. `ReviewPart` likewise has no composed name. The search index `part_fts` holds the components (custom name, instrument names, voices) plus every key spelling of the shipped languages (`musicus_i18n::KEY_SPELLINGS`, exported from the catalogue at build time), and `fts_terms` drops the connecting words of every shipped language (`NAME_CONNECTORS`, English "in") unless the query has nothing else. The default annotation set is stored with `name = NULL`; the client shows "My annotations" in its language.
- Why: a-data-as-entered; the server cannot know the reader's language (no setting, R-I18N-001). Key spellings are search aliases, never shown, so the server still emits no prose.
- Serves: R-I18N-006, R-I18N-007, R-LIB-010, R-LIB-011, R-LIB-004, R-ANN-009.
- State: implemented (phase 3): `naming::NameWords`, `musicus_i18n::name_words`, client `names.rs`, `Part.instruments`, `part_index::index_text`, `library::fts_terms`, migration 0009 (table rebuild with foreign keys off; partial unique index keeps one unnamed set per part, as the old NOT NULL + UNIQUE did for "My annotations"); with German (a-i18n-german) the index holds e.g. "Horn Eb E♭ Es" (migration 0010 rebuilds it).
- If you change it: a new shipped language adds its key names and connector words to the index only after a `part_fts` rebuild (new migration `DELETE FROM part_fts`, as 0010 for German); `part_index::tests::indexed_key_spellings_are_current` fails until `INDEXED_SPELLINGS` is updated with it.

### a-browser-sort
- Decision: every displayed list of names is sorted in the browser with one collator: `locale::compare` (and `locale::sort_by_text`) = `Intl.Collator(<UI language>, {numeric: true, sensitivity: "base"}).compare`, created once per page load, ties broken by code point (it lives in `locale`, not `ui`, so all language logic stays in one module). Server `ORDER BY … COLLATE NOCASE` stays only as a stable default and `LIMIT 1000` as a safety cap (not paging). `naming::natural_cmp` stays in shared code where a deterministic, locale-free order is needed (tests, import grouping).
- Why: the browser has the collation data for free; `icu_collator` in WASM would add ≈ 418 KB gzip (d-i18n-fluent). SQLite NOCASE folds ASCII only ("Ö" after "Z").
- Serves: R-I18N-008.
- State: implemented (phase 5); the lists and their order are in `web-client.md` § Sorting.
- If you change it: libraries above 1000 matching scores need real paging, then the server must sort by the same rules (ICU collation) — ask the user.

### a-intl-js
- Decision: the UI language is the first entry of `navigator.languages` that Musicus ships (compared by primary subtag), else English; there is no setting and nothing is stored (R-I18N-001, R-SEC-001). It is decided once at start in the client module `locale` (also sets `<html lang>`). Dates, times and numbers are formatted by the browser's `Intl.DateTimeFormat` / `Intl.NumberFormat` with that language through `js_sys` (0 KB of WASM data), replacing `toLocaleDateString("default")` and the hand-made `format_size`.
- Why: the user's decision (no per-user language, browser language, English fallback); formatting libraries in Rust would add hundreds of KB (d-i18n-fluent). With "default" the date followed the browser while the text would follow the catalogue language; one language for both keeps them consistent.
- Serves: R-I18N-001, R-I18N-002, R-I18N-005, R-SEC-001.
- State: implemented (phase 1, `crates/musicus-web/src/locale.rs`).
- If you change it: a language setting would need storage on the server (never on the device, R-SEC-001) and a requirement change; keep all locale logic inside `locale` (a desktop app would replace only that module).

## Updates, downgrades and platforms (F-OPS-005, F-OPS-006, F-OPS-007, F-TECH-002)

Plan: `docs/technical/chapters/14-updates.typ` (label `updates`). **Nothing here is implemented yet**; phase 0 of the
plan's roadmap must ship in 1.0.0. Each entry has a **State** line.

### d-update-model
- Decision: every database change has an *additive* part (new tables/columns, copy of existing data into them; runs at the first start of the release), a *rebuild* step (refills the new structures from the old ones; runs at start when an older release used the database since the additive part ran) and a *removal* part (drops/rewrites; run later by a newer release only when no release inside the downgrade window needs the old structures). Downgrade window: the 3 previous minor releases, the release the installation was upgraded from for 3 months after that upgrade, and all patches of the same minor (R-OPS-014). While a removal is pending, the newer release keeps the old structures up to date (writes both). The database records `last_version`, `oldest_allowed` and the upgrade history (proposed tables `musicus_meta`, `musicus_updates`, `musicus_upgrades`); an older release starts if `own >= oldest_allowed`, else exits unchanged naming the oldest possible release (R-OPS-015). Running a removal raises `oldest_allowed` to the release that brought it. Data that only the newer release can represent is lost on a downgrade and not restored on re-upgrade (R-OPS-016). Changes that restrict access count as removals: mirrored into the old data, or they raise `oldest_allowed` when first used (R-OPS-017; interpretation, see the plan). Every start with pending work takes a `VACUUM INTO` snapshot under `snapshots/` (keep 3) and restores it on failure (R-OPS-019, R-OPS-020); `backup` never applies updates (R-OPS-021); new `restore` command (R-OPS-022). A small own update runner replaces `sqlx::migrate!` (which refuses unknown or merged migrations). Release updates are kept in the program forever (upgrade from any release, R-OPS-011), so removal SQL lives in the update files, not in the database.
- Why: the user's decisions (volunteer admins; going back = start the older image; no data loss except explicit feature removal, restored on downgrade; new-only data may be lost). Down-migration scripts per change were rejected: they would have to be written and tested for every change and would run in the newer program anyway. A downgrade by backup restore loses everything since the update. An earlier draft concept (expand/contract with a one-minor window and a portable archive format) is superseded by this one.
- Serves: F-OPS-005, R-OPS-010, R-OPS-011, R-OPS-012, R-OPS-013, R-OPS-014, R-OPS-015, R-OPS-016, R-OPS-017, R-OPS-019, R-OPS-020, R-OPS-021, R-OPS-022.
- State: planned (phase 0). Today: `sqlx::migrate!` in `Db::open` for every command (`db.rs`), `backup` migrates first (`main.rs`), forward-only migrations 0001–0010 (collapsed into the 1.0 baseline at the first release).
- If you change it: the window is a user decision (requirements R-OPS-014, R-OPS-016) — ask first. Never write a removal into the additive part; an access restriction is never additive. Every stored enum read from the database needs a fallback for unknown values before a newer release may write new values (error codes already have `#[serde(other)]`).

### a-preview-migrations
- Decision: during development each branch that changes the database adds a preview update `crates/musicus-server/migrations/preview/NNNN-<topic>.sql` (additive and removal sections), applied by development builds so local demo databases keep working. A merged preview is never edited again (correction = new preview). At each release a script merges all previews into `migrations/<version>.sql`, lists the contained previews in its header and deletes the preview files; a database that applied all listed previews marks the release update as applied without running it, one that applied only some is reported as an unclear development database. Release builds refuse to start with preview files (R-OPS-024). Existing developer databases (`_sqlx_migrations` with 0001–0010) are recognised once as having the 1.0 baseline. Details (paths, header syntax) are design to be confirmed during implementation.
- Why: the user's decision (developers keep demo databases; one database update per release); `sqlx::migrate!` cannot merge migrations.
- Release pull requests come from branches `release/<version>`; only they may delete previews and add the merged update (checked by a-db-check).
- Serves: R-OPS-023, R-OPS-024, R-OPS-040.
- State: planned (phase 0). Until then: add `000N_<name>.sql` as before (workflows.md § Change the schema).
- If you change it: dev databases must still survive a release merge; release builds must never contain previews.

### a-patch-no-db-changes
- Decision: patch releases (X.Y.Z, Z > 0) contain no database update and write no new kinds of stored values (a new role, region kind, ink format version, error code stored in `import_files.problem`), because an earlier patch of the same minor must read their data; old minors get no new patches (fixes come with the next release); release rhythm irregular. Release checklist line: "patch: no new stored values".
- Why: the user's decision; makes "any patch of the same minor, in both directions" (R-OPS-014 c) hold without any mechanism.
- Serves: R-OPS-018, R-OPS-041, R-OPS-014.
- State: planned; database updates enforced by the release check (a-db-check, phase 0); new stored values only by the upgrade/downgrade test matrix, fallbacks for unknown values on every stored enum (as `ErrorCode` has, phase 0) and the checklist.
- If you change it: a database change or a new stored value in a patch would break rule (c) of the downgrade window.

### a-db-check
- Decision: three CI checks, built together with phase 0 (new runner + preview folder), not earlier. (1) Database check, blocking: in a normal pull request (branch not `release/<version>`) fails if a database update file is added outside `migrations/preview/`, if a released update or a preview already on `main` is modified or deleted (changes go into a new preview), or if the schema snapshot (proposed `crates/musicus-server/schema.sql`, generated by `scripts/db-schema.sh` from all updates applied to an empty database) is outdated or changed without a new preview. (2) `db-change` notice, non-blocking: a pull request adding a preview gets the label `db-change` and the note "This PR changes the database; the next release must be a minor release (x.Y.0), not a patch." (3) Release check, blocking, on `release/<version>`: a patch version fails with any preview or a database update for this version; a minor/major version's merged `migrations/<version>.sql` must list exactly the removed previews and no preview may remain. Release pull requests are recognised by the branch name only.
- Why: the user's decisions; makes R-OPS-018/R-OPS-024 enforceable before publishing, and the snapshot diff shows schema changes in review. Code expecting a missing column is caught by the `test` job. Limitation: new kinds of stored values in existing columns are invisible to these checks (a-patch-no-db-changes, R-OPS-041).
- Serves: R-OPS-038, R-OPS-039, R-OPS-040, R-OPS-018, R-OPS-024.
- State: planned (phase 0).
- If you change it: the branch name `release/<version>` is the only release marker; renaming it needs the check and `workflows.md` changed together.

### a-no-update-check
- Decision: Musicus never contacts the internet to look for newer releases; no setting enables it.
- Why: the user's decision; servers on networks without internet, nothing about the ensemble sent anywhere.
- Serves: R-OPS-025.
- State: holds today (no such code exists); keep it that way.
- If you change it: requirement change with the user's consent.

### a-version-public
- Decision: the version is visible to everyone: public `GET /api/version` (no login), an "About" view for every role, `--version`; the user guide and the technical documentation show it on the title page and the guide site on every page; the requirements repository gets a git tag `vX.Y.Z` per release (no version in its files).
- Why: the user's decision; the version is needed to pick the guide, read release notes and know the downgrade targets. Fingerprinting risk accepted (only the number is revealed).
- Serves: R-OPS-033, R-OPS-034, R-OPS-035, R-OPS-036.
- State: planned (phase 0). Today: `--version` only; title pages carry a hand-written "0.1"; the guide site's version bar shows the tag (a-guide-site).
- If you change it: hiding the version needs a requirement change.

### a-unencrypted-export
- Decision: backups, snapshots and exports are written unencrypted, like the database (plain SQLite; passwords only as Argon2 hashes, d-hashing); the user guide states they need the same care as the data directory.
- Why: the user's decision; encryption would need key management that volunteers cannot be expected to handle, and the database itself is not encrypted.
- Serves: R-OPS-037.
- State: holds today for `backup`; the guidance text in the user guide is still missing.
- If you change it: R-ANN-018 (encrypted annotations at rest) would stay encrypted inside the copies; encrypting the copies themselves needs a requirement change.

### a-platforms
- Decision: supported: Docker Compose (preferred; linux/amd64 and linux/arm64 images, also for Docker Desktop on Windows/macOS) and the plain program on Linux (x86_64, arm64), Windows (x86_64) and macOS (arm64); Raspberry Pi 3/4/5 or newer with a 64-bit OS only. Images are published per release with version tags; the Compose file uses a version tag. Proposed, not decided: program downloads per platform (R-OPS-029), the web client embedded in the program (R-OPS-030), the OS-usual data folder as default (R-OPS-031), CI on all platforms (R-OPS-032). Storage, file store and PDF engine get owned module boundaries with conformance suites, no second implementation (F-TECH-002; R-TECH-001 unchanged).
- Why: the user's decisions (volunteer admins, Pi support, plain program must stay possible on all three systems; database/S3 replacement only prepared).
- Serves: F-OPS-006, R-OPS-026, R-OPS-027, R-OPS-028, F-TECH-002, R-TECH-004, R-TECH-005, R-TECH-006, R-TECH-007.
- State: planned. Known blocker: `blobs.rs::sync_dir` opens the blob directory with `File::open(dir)?.sync_all()`, which fails on Windows (every upload would fail); other findings in `status.md` and the plan.
- If you change it: dropping a platform or 32-bit support needs the user's consent; keep platform-specific code behind `cfg` with a working fallback.
