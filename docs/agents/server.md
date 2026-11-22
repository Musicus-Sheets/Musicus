# Server (`crates/musicus-server`)

## Module graph

`main` → `config` (clap) → `lib::open_state` → `state::App {db, blobs, cache, settings, limiter, hashing, layout_jobs}`.
`routes::app(state, web_dir)` → handlers in `routes::*` use `auth::AuthUser`, `access::part_visible!`, `error::AppError`,
`db::{now, json_ids}`, `blobs`, `cache` (files only), `processors` (upload only). `layout` (system detection, own
module doc `layout.md`) runs in a background worker, not in handlers. `tests/api.rs` drives `routes::app` directly.

## Start-up (`main.rs::serve`)

1. `ServeArgs` → `Settings {cookie_secure = !insecure_cookies, client_cache, session_ttl_secs = days*86400, invite_ttl_secs = 7d, max_upload_bytes, trust_proxy}`.
2. `open_state(data_dir, ram_cache_mb*MiB, settings)`: mkdir, `Db::open` (readers = cores clamped 2..8; the update runner `updates::{check_build, inspect, apply}`: release build with previews ⇒ exit (R-OPS-024), program older than `oldest_allowed` ⇒ exit with "This data was last used by Musicus X. Musicus Y can no longer use it; start Musicus Z or newer." and nothing written (R-OPS-015), else rebuilds / additive parts / due removals, each in its own transaction, then the marker; a-update-runner), log lines of the report, `part_index::rebuild_if_needed` (rebuilds `part_fts` when another program used the database last or its row count differs from `parts`), `BlobStore::open` (mkdir `blobs/`, `tmp/`, empties `tmp/`), `App::new` (semaphore permits = cores).
3. `bootstrap_admin`: if `users` empty → insert `admin` (role admin, no password) + invite; log `<public_url>/invite/<token>` (R-ACC-003).
3a. `layout::worker::spawn(state)`: background system detection of every file without a current layout (backfill after updates, then new uploads; a-layout-worker).
4. `--web-dir` must contain `index.html` else exit error.
5. TLS if `--tls-cert` and `--tls-key` (rustls with `ring` provider installed explicitly) else plain TCP. `into_make_service_with_connect_info::<SocketAddr>()` (needed by `ClientIp`).
6. Graceful shutdown on SIGTERM/Ctrl-C (R-OPS-008).

`--version` prints `musicus-server <musicus_api::VERSION>` (clap `version = musicus_api::VERSION`, a-version-source; R-OPS-033). Other subcommands: `create-admin --username U [--display-name D]` (prints invite URL), `backup --to FILE` (`VACUUM INTO`, refuses existing file; R-OPS-005), `healthcheck [--listen]` (TCP connect to listen addr, 0.0.0.0→127.0.0.1, `GET /api/health HTTP/1.0`, exit 0 iff status line ends with ` 200`; R-OPS-007).

## Configuration (`config.rs`) — all `serve` options

| Option / env | Default | Effect |
| --- | --- | --- |
| `--data-dir` / `MUSICUS_DATA_DIR` | `data` | db + blobs + tmp |
| `--listen` / `MUSICUS_LISTEN` | `0.0.0.0:8080` | bind address |
| `--web-dir` / `MUSICUS_WEB_DIR` | none | built client; none ⇒ API only |
| `--public-url` / `MUSICUS_PUBLIC_URL` | `http://localhost:8080` | printed links only |
| `--tls-cert`, `--tls-key` / `MUSICUS_TLS_*` | none | built-in HTTPS |
| `--insecure-cookies` / `MUSICUS_INSECURE_COOKIES` | false | drop cookie `Secure` |
| `--client-cache` / `MUSICUS_CLIENT_CACHE` | `none` | `none` ⇒ `private, no-store`; `immutable` ⇒ `private, max-age=31536000, immutable` (R-FILE-007) |
| `--ram-cache-mb` / `MUSICUS_RAM_CACHE_MB` | 128 | `FileCache` capacity |
| `--max-upload-mb` / `MUSICUS_MAX_UPLOAD_MB` | 200 | upload limit (413) |
| `--session-days` / `MUSICUS_SESSION_DAYS` | 60 | sliding session TTL |
| `--trust-proxy` / `MUSICUS_TRUST_PROXY` | false | `ClientIp` uses first `X-Forwarded-For` |
| `RUST_LOG` | `info,tower_http=warn,sqlx=warn` | tracing filter |

## AuthUser extractor (`auth.rs`) — runs for every handler with an `AuthUser` param

1. Token: `Authorization: Bearer <t>` else cookie `musicus_session=<t>`; none ⇒ 401 (+ `WWW-Authenticate: Bearer`).
2. Cookie + method ∉ {GET, HEAD, OPTIONS} + no `x-musicus-csrf` header ⇒ 403 (R-SEC-004).
3. `SELECT … FROM sessions s JOIN users u … WHERE token_hash = sha256(t) AND expires_at > now AND disabled = 0` on read pool ⇒ else 401.
4. If `now - last_seen_at > 3600` ⇒ UPDATE `last_seen_at`, `expires_at = now + ttl` on write pool.
5. Returns `AuthUser {id, role, session_id}`; helpers `is_librarian()`, `require_librarian()`/`require_admin()` ⇒ 403.

`LoginLimiter`: in-memory `HashMap<key, Vec<ts>>`, keys `user:<lowercased name>` and `ip:<addr>`; ≥10 failures in 15 min ⇒ 429 `too_many_attempts {minutes}`; success clears the user key only; map pruned above 10 000 keys. Resets on restart (R-SEC-003).

## Endpoint table (all under `/api`)

Auth column: `—` none, `U` any logged-in user, `L` librarian/admin, `A` admin. "ACL" = filtered/checked with `part_visible!`. Denied ACL ⇒ 404.

| Method | Path | Auth | Handler | Behaviour / status codes | Requirements |
| --- | --- | --- | --- | --- | --- |
| GET | `/health` | — | `routes::health` | `SELECT 1` on read pool ⇒ 200 `ok` | R-OPS-007 |
| GET | `/version` | — | `routes::version` | 200 `VersionInfo {version}` = `musicus_api::VERSION` (workspace version), `Cache-Control: no-store`; nothing else (a-version-public) | R-OPS-033 |
| POST | `/auth/login` | — | `auth::login` | `LoginRequest{username,password,bearer}`; limiter 429; 401 wrong, disabled or a role this release does not know (also: such a session ⇒ 401 in `AuthUser`, its invite link ⇒ 404); 200 `LoginResponse` + Set-Cookie (or `token` if bearer) | R-ACC-004, R-SEC-003 |
| POST | `/auth/logout` | U | `auth::logout` | deletes current session; 204 + clearing cookie | R-ACC-006 |
| GET | `/auth/me` | U | `auth::me` | `CurrentUser` | — |
| PUT | `/auth/reader-settings` | U | `auth::set_reader_settings` | `ReaderSettings {mode: page\|systems, staff_size: S\|M\|L\|XL}` (invalid ⇒ 4xx from the JSON extractor); stored in `users`; returns it; also part of `CurrentUser.reader` | R-READ-014 |
| POST | `/auth/password` | U | `auth::change_password` | wrong current ⇒ 403; policy 8..256 chars ⇒ 400; deletes other sessions; 204 | R-ACC-007 |
| GET | `/auth/sessions` | U | `auth::list_sessions` | own non-expired sessions, `current` flag | R-ACC-006 |
| DELETE | `/auth/sessions/{id}` | U | `auth::revoke_session` | own only else 404; 204 | R-ACC-006 |
| GET | `/auth/invites/{token}` | — | `auth::invite_info` | valid+not expired+user enabled ⇒ `InviteInfo` else 404 | R-ACC-002 |
| POST | `/auth/invites/{token}` | — | `auth::accept_invite` | sets password, deletes invites+sessions, new session cookie; 200 `LoginResponse` | R-ACC-002 |
| GET | `/users` | A | `users::list` | `Vec<UserSummary>` | R-ACC-009 |
| POST | `/users` | A | `users::create` | username lowercased, chars `[alnum._-]`, ≤64; dup ⇒ 409; role `unknown`/unknown name ⇒ 400 `in_use_or_unknown`; returns `CreatedUser{user, invite}` | R-ACC-001 |
| PATCH | `/users/{id}` | A | `users::update` | `UpdateUser`; self-disable / self-demote ⇒ 409; unknown role ⇒ 400 `in_use_or_unknown` (a stored unknown role is kept unless a known one is set); disable deletes sessions+invites | R-ACC-008 |
| POST | `/users/{id}/invite` | A | `users::reinvite` | disabled ⇒ 409; replaces invites | R-ACC-010 |
| GET | `/instruments` | U | `groups::list_instruments` | all | — |
| POST | `/instruments` | L | `groups::create_instrument` | name ≤80 unique (NOCASE) ⇒ 409 | R-LIB-003 |
| PUT/DELETE | `/instruments/{id}` | L | `groups::update_instrument` / `delete_instrument` | rename re-indexes the instrument's parts (`part_fts`); delete in use ⇒ 409 (FK RESTRICT, see Errors) | R-LIB-003, R-LIB-010 |
| GET | `/groups` | L | `groups::list_groups` | `Vec<Group>` with member/instrument/score ids | R-ACL-004 |
| POST | `/groups` | A | `groups::create_group` | | R-ACL-004 |
| PUT/DELETE | `/groups/{id}` | A | `groups::rename_group` / `delete_group` | | R-ACL-004 |
| PUT | `/groups/{id}/members` | A | `groups::set_members` | `IdList`, replaces; unknown id ⇒ 409 (FK) | R-ACL-004 |
| PUT | `/groups/{id}/instrument-grants` | L | `groups::set_instrument_grants` | replaces | R-ACL-001, R-ACL-004 |
| PUT | `/groups/{id}/score-grants` | L | `groups::set_score_grants` | replaces | R-ACL-001, R-ACL-004 |
| GET | `/scores?q=&genre=&tag=` | U, ACL | `library::list_scores` | `fts_terms`: one quoted prefix term per word (max 10, deduped) as JSON array; every term must match `score_fts` (incl. `archive_search`) or the `part_fts` name of a part the user may read (`json_each` CROSS JOIN fts, UNION, `GROUP BY` score `HAVING COUNT = terms`); genre/tag NOCASE; members: scores with ≥1 visible part, parts filtered; limit 1000 | R-LIB-004, R-LIB-005 |
| GET | `/tags` | U, ACL | `library::list_tags` | every tag of the scores the user can see (librarians: all), merged ignoring case, sorted by `natural_cmp`; for suggestions | R-LIB-014 |
| POST | `/scores` | L | `library::create_score` | `ScoreInput` validated (title ≤200, ≤30 tags ≤40 chars, dedupe NOCASE, archive number ≤40 optional, not unique) | R-LIB-001, R-LIB-015 |
| GET | `/scores/{id}` | U, ACL | `library::get_score` | 404 if no visible part (members) | R-LIB-005 |
| PUT/DELETE | `/scores/{id}` | L | `library::update_score` / `delete_score` | update writes `archive_search` with the number; delete cascades parts, versions, annotation sets (blobs stay) | R-LIB-001, R-LIB-015 |
| POST | `/scores/{id}/parts` | L | `library::create_part` | `PartInput` (label optional ≤120, voices: list cleaned by `naming::clean_voices`, ≤10 entries of ≤20 chars else 400, stored in `part_voices` by `set_voices`, key `naming::normalize_key` else 400, ≤20 instruments); re-indexes `part_fts` | R-LIB-002, R-LIB-010, R-LIB-011 |
| GET | `/parts/{id}` | U, ACL | `library::get_part` | `Part` with its name components (`label`, `instruments` {id, name}, key, voices; no composed name) and current `FileVersion` | R-LIB-005, R-LIB-010 |
| PUT/DELETE | `/parts/{id}` | L | `library::update_part` / `delete_part` | all fields + instruments + voices replaced; key as on POST, except the part's stored (legacy) key may be sent unchanged; re-indexes `part_fts` (delete: trigger) | R-LIB-002, R-LIB-011 |
| PUT | `/parts/{id}/file` | L | `library::upload_file` | raw body; no body limit layer (blob store enforces `max_upload_bytes` ⇒ 413); non-PDF ⇒ 415; dedupe; new version unless same file as latest | R-FILE-001, R-FILE-002, R-FILE-003 |
| GET | `/parts/{id}/versions` | U, ACL | `library::list_versions` | newest first | R-FILE-002 |
| GET | `/files/{sha256}` | U, ACL(any part) | `files::get_file` | invalid hash ⇒ 404; If-None-Match ⇒ 304; Range ⇒ 206 via ServeFile; else RAM cache; ETag `"sha"`; Cache-Control per setting; Last-Modified removed | R-FILE-005, R-FILE-006, R-FILE-007 |
| GET | `/files/{sha256}/thumbnail` | U, ACL(any part) | `files::get_thumbnail` | PNG; 404 if none | R-LIB-006 |
| GET | `/files/{sha256}/layout` | U, ACL(any part) | `files::get_layout` | `FileLayout`: no `file_layouts` row ⇒ `state: pending, algorithm: 0, pages: []`; else `done` + stored pages (an older algorithm is served until re-analysed) with `layout_corrections` applied (`apply_correction`, `corrected: true`); `failed`, an unknown state or unreadable pages ⇒ `failed`, `pages: []` (data-model.md § Stored values); invalid/unknown/denied ⇒ 404 | R-READ-010, R-READ-016 |
| PUT | `/files/{sha256}/layout/pages/{page}` | L | `files::put_correction` | `PageCorrection` (`validate` ⇒ 400, incl. an unknown band kind ⇒ `in_use_or_unknown`); not analysed/failed ⇒ 409; page ∉ 0..pages ⇒ 404; upsert `layout_corrections` (`updated_by`); returns the effective `FileLayout` | R-READ-016 |
| DELETE | `/files/{sha256}/layout/pages/{page}` | L | `files::delete_correction` | removes the correction (back to automatic); same checks; returns the effective `FileLayout` | R-READ-016 |
| GET | `/import/batches` | L | `import::list_batches` | newest first, `ImportBatch` with file counts (incl. unmatched = no item) and pieces/committed counts | R-LIB-016 |
| POST | `/import/batches` | L | `import::create_batch` | `NewImportBatch` (name ≤200, pattern must parse ⇒ else 400, language de/en) | R-LIB-016, R-LIB-017 |
| GET | `/import/batches/{id}` | L | `import::get_review` | `ImportReview`: batch, `ReviewPiece`s (resolved now), unmatched and duplicate files; 404 unknown | R-LIB-019 |
| PUT | `/import/batches/{id}` | L | `import::update_batch` | `UpdateImportBatch`; new pattern/language ⇒ `repropose` (piece changes kept by grouping key, part changes by path) | R-LIB-017, R-LIB-018 |
| DELETE | `/import/batches/{id}` | L | `import::delete_batch` | staging rows only; committed pieces, files, blobs stay; 204 | R-LIB-016 |
| POST | `/import/batches/{id}/files` | L | `import::register_files` | `RegisterImportFiles` (≤20 000, body ≤16 MiB; path: no `..`/empty segments, `.pdf`; sha256 hex) ⇒ sorted by path, upsert (changed hash ⇒ re-staged unless committed), duplicate if content is in the library (`part_files`) or under another path of the batch, `uploaded` if the blob already exists, else `pending`; `repropose`; ⇒ `RegisteredImportFiles` (pending/failed to upload) | R-LIB-016 |
| PUT | `/import/files/{id}/content` | L | `import::upload_content` | raw body like `upload_file` (limit 413); sha ≠ announced ⇒ 422 + state failed; non-PDF ⇒ 415 + failed; already uploaded ⇒ no-op; content meanwhile in the library ⇒ duplicate; ⇒ `StagedFile` | R-LIB-016, R-FILE-001 |
| GET | `/import/files/{id}` / `/import/files/{id}/thumbnail` | L | `import::get_staged_file` / `get_staged_thumbnail` | staged PDF / PNG through the batch (not via `/files/{sha}`), ETag etc. as `serve_blob`; 404 if not uploaded / no thumbnail | R-LIB-016, R-LIB-019 |
| PATCH | `/import/items` | L | `import::update_items` | `UpdateImportItems` (1..5000 ids of one batch; committed ⇒ 409; an item of a newer release (unknown kind/state, also of its piece, or unreadable overrides) ⇒ 400 `unknown_items`; target score must exist, instruments: ids of `PartOverrides.instruments` exist (`json_each`), ≤20 entries, kept words ≤120 chars (a-import-instruments), key via `normalize_key` ⇒ stored as code, else 400; voices cleaned by `naming::clean_voices`, ≤10 of ≤20 chars, else 400; label ≤120); merge or `reset`; ⇒ changed pieces' `ReviewPiece`s | R-LIB-019 |
| POST | `/import/batches/{id}/commit` | L | `import::commit` | `CommitImport`: listed pieces (each `can_commit`, else error entry `import_blocked {note}` / `import_committed`) or all `is_green`; one write tx per piece (score or confirmed existing piece, parts, version 1, reindex, items committed) ⇒ `CommittedImport` | R-LIB-021 |
| POST | `/import/batches/{id}/undo` | L | `import::undo` | 409 (nothing removed) if an imported part has annotation sets, a created piece has other parts, or is in a programme; else one tx: delete imported parts + created pieces, items back to pending ⇒ `UndoneImport` | R-LIB-021 |
| GET / PUT / DELETE | `/import/aliases` (DELETE `?alias=`) | L | `import::list_aliases` / `put_alias` / `delete_alias` | alias stored as `alias_key` (1..80 chars); unknown instrument ⇒ 400; upsert; delete unknown ⇒ 404 | R-LIB-020 |
| GET | `/programmes` | U, ACL | `programmes::list` | `item_count` = visible items; members: only programmes with ≥1 visible item | R-PROG-002 |
| POST | `/programmes` | L | `programmes::create` | date `YYYY-MM-DD` else 400 | R-PROG-001 |
| GET | `/programmes/{id}` | U, ACL | `programmes::get` | items with visible scores only (positions kept); member with none ⇒ 404 | R-PROG-002 |
| PUT/DELETE | `/programmes/{id}` | L | `programmes::update` / `delete` | | R-PROG-001 |
| PUT | `/programmes/{id}/items` | L | `programmes::set_items` | `IdList` of score ids (duplicates allowed, ≤200); position = array index | R-PROG-001 |
| GET | `/parts/{id}/annotation-sets` | U, ACL, owner | `annotations::list` | own sets, default first | R-ANN-008, R-ANN-014 |
| POST | `/parts/{id}/annotation-sets` | U, ACL | `annotations::create` | `NewAnnotationSet{name, programme_id?, copy_from?}`; bound to current file (or source set's file); no file ⇒ 409; first set of user+part ⇒ default; copy copies pages with revision 1; name dup ⇒ 409 | R-ANN-008, R-ANN-016 |
| PATCH | `/annotation-sets/{id}` | U, owner | `annotations::update` | name; `programme_id` absent/null/id (`double_option`); `is_default:true` clears others | R-ANN-008 |
| DELETE | `/annotation-sets/{id}` | U, owner | `annotations::delete` | cascades pages | R-ANN-008 |
| POST | `/annotation-sets/{id}/migrate` | U, owner | `annotations::migrate` | to current file version if page counts equal and known else 409 | R-ANN-016 |
| GET | `/annotation-sets/{id}/pages/{page}` | U, owner, ACL | `annotations::get_page` | 200 octet-stream (zstd-decoded ink) + ETag `"rev"`, `no-store`; never saved ⇒ 204 + ETag `"0"` | R-ANN-013, R-ANN-014 |
| PUT | `/annotation-sets/{id}/pages/{page}` | U, owner, ACL | `annotations::put_page` | body limit 4 MiB; `If-Match` required (428), non-numeric ⇒ 400; page out of `0..page_count` (or 2000 if unknown) ⇒ 404; `ink::decode` fail ⇒ 422; revision mismatch ⇒ 409 `PageConflict`; else upsert rev+1, 200 `SavedPage` + ETag | R-ANN-006, R-ANN-012 |
| any | other `/api/*` | — | fallback | JSON 404 | — |

`own_set` = `WHERE s.id = ?1 AND s.user_id = ?2` + `ensure_part_readable` (INV-9).

## Errors (`error.rs`)

Body `ApiError {error, code, args, message}` (format and code list: `api-and-ink.md`; a-error-codes, R-I18N-004). The server builds no text for users: `AppError` variants take an `ErrorCode`, `message` is always the code's English `Display`.
`AppError` variants → status/class: NotFound 404 `not_found` (code `not_found`); Unauthorized 401 (`unauthorized`); Forbidden(code) 403; BadRequest(code) 400; Conflict(code)/ConflictWith 409 (ConflictWith sends a custom JSON body, used for `PageConflict`); Unprocessable(code) 422; PayloadTooLarge(max) 413 (`payload_too_large {max_bytes}`); UnsupportedMediaType(code) 415; PreconditionRequired(code) 428; TooManyRequests(minutes) 429 (`too_many_attempts {minutes}`); Internal(detail) 500 (`internal`; detail only logged, R-SEC-008). `AppError::code()` gives the code (also used for `ImportItemError` and `import_files.problem`).
`From<sqlx::Error>`: RowNotFound → 404; unique violation → 409 `already_exists`; FK violation → 409 `in_use_or_unknown` (also `ON DELETE RESTRICT`, which SQLite reports as extended code 1811 `SQLITE_CONSTRAINT_TRIGGER` and sqlx does not classify: `is_restrict_violation`); else 500.
`From<io::Error>` → 500. Validators: `required(Field, value, max_chars)` (trim, non-empty ⇒ `field_empty`), `optional(...)` (empty → None), both `field_too_long {field, max}` (`too_long`). Upload body errors: `upload_interrupted` (detail logged at debug).

## Middleware (`routes::app`)

Overriding headers `X-Content-Type-Options: nosniff`, `X-Frame-Options: DENY`, `Referrer-Policy: same-origin`; `Permissions-Policy: camera=(), microphone=(), geolocation=()` if not present (R-SEC-005). `TraceLayer`. Axum default body limit 2 MB except routes above. Web dir: `ServeDir(dir).precompressed_br().precompressed_gzip().fallback(ServeFile(index.html))`.

## Upload pipeline (`library::upload_file` → `store_upload` = `sniff_pending` + `store_file` + `add_version`; the import's `upload_content` uses the first two)

part exists? (404) → `blobs.receive(body, limit)` → read first 1 KiB → `sniff_mime` (415) → known sha? commit (dedupe) : read file, `spawn_blocking(process_pdf)` (panic ⇒ no metadata), thumbnail via `put_bytes`, commit, `INSERT … ON CONFLICT(sha256) DO NOTHING`, `layout_jobs.notify_one()` (new file only) → tx: latest version's file ≠ this file ⇒ insert version n+1. On any error the pending temp file is discarded.

## Tests (`tests/api.rs`)

`Server::start()` = temp dir + `open_state` + `bootstrap_admin` + `routes::app(state, None)`; `cookie_secure=false`; keeps `state` so tests drive `layout::worker::run_once` (no worker task in tests). Helpers: `send`, `json`, `ok::<T>`, `login` (bearer), `accept_invite` (returns cookie), `admin()`, `member()`, `upload()`; `hits(q, token)` (number of search results); `library()` fixture = instruments Trumpet/Flute, group Trumpets with anna, score with trumpet part (no label, key Bb, voice "2" ⇒ "Trumpet in B♭ 2", 4 pages) + flute part (custom name "Solo Flute", 2 pages), both uploaded. `tiny_pdf(pages, tag)` builds valid PDFs. Tests: `login_sessions_and_csrf`, `members_only_reach_granted_parts`, `uploads_are_versioned_and_validated`, `annotations_are_private_versioned_and_conflict_checked`, `programmes_and_search`, `user_administration`, `instruments_are_listed_for_everyone_and_kept_while_in_use`, `part_names_keys_and_part_search` (incl. German key names: "horn es", "horn in es", "trumpet in b 1"), `migration_0002_keeps_old_parts` (applies only 0001 from a temp copy, writes old-style rows, then `open_state`; also checks the score index rebuilt by 0006), `archive_numbers_are_free_text_searchable_and_not_unique` (trim, duplicates allowed, search "304"/"0304"/"A12"/"a-12", old number leaves the index, blank ⇒ none, 41 chars ⇒ 400, member finds only visible pieces), `system_layouts_are_detected_in_the_background_and_follow_file_access` (`staff_pdf` uploads: pending → worker → 8 systems per page, empty pages whole, ACL 404, reader settings round trip), `librarians_correct_system_layouts_for_everyone` (409 before analysis, member 403, page 404, invalid 400, merge/hide/whole page visible to members, kept after a re-analysis with a raised version, reset), `import_stages_reviews_commits_and_undoes` (member 403, invalid pattern 400, duplicates by content (library, twice in folder), hash mismatch 422, resume uploads only missing files, staged files 404 via `/api/files` for librarian and member, own file/thumbnail routes, German key B, red unknown instruments block commit, Partitur/Direktion created and mapped separately, abbreviation yellow ⇒ alias green, existing piece suggested + confirmed, commit all green, member reads committed part, committed items locked 409, undo refused while annotated, undo removes created piece + parts but keeps the existing piece, files stay, alias delete, batch delete), values written by a newer release (helper `as_newer_release`: statements with `PRAGMA ignore_check_constraints`): `an_unknown_role_cannot_log_in_and_is_never_written_back`, `layouts_of_a_newer_release_show_whole_pages_and_are_not_overwritten`, `annotation_pages_of_a_newer_format_are_served_unchanged`, `import_items_and_files_of_a_newer_release_are_left_out` (data-model.md § Stored values), `import_of_flat_numbered_files_groups_by_number_and_keeps_changes` (304/0304 grouped, title missing red, no key assumed, English "B" ambiguous, bulk genre, ambiguous key override 400, two instruments + two voices override (unknown id 400, 21 instruments 400) committed as a part with both, voices override as a list (11 ⇒ 400, "1 & 2" ⇒ two voices, committed as `Part.voices`), language change keeps changes, archive number on the created piece, layout cross-check Partitur with one staff, second batch skips library content and re-uses known blobs).
