# Status

Source of truth for requirement status: `docs/requirements` (`status` field; overview in `docs/requirements/index.md`). This file adds what the requirements do not say: gaps, debt, verification limits.

## Implemented (version 0.1.0)

Everything in `server.md` endpoint table and `web-client.md`; requirements with status `implemented`/`verified`.

## Not implemented (requirements with status `proposed`)

| Requirement(s) | Topic | Notes for implementers |
| --- | --- | --- |
| R-ACL-005, R-ACL-006 | grants per voice / per programme / expiry | extend `part_visible!()` (d-access); new tables + migration; per voice: join `part_voices` (indexed on `voice`, a-part-voices) |
| R-LIB-007 | extended score metadata (the archive number is done: R-LIB-015) | migration + `ScoreInput`/`Score` + `pages/forms.rs::ScoreFields` + FTS columns if searchable (FTS5 needs drop + recreate + rebuild, see 0006) |
| R-FILE-008 | MusicXML | new processor in `processors.rs`; `sniff_mime`; annotations need musical anchors (not page coords) — design first |
| R-FILE-009 | GC of unreferenced blobs | must respect backup consistency (grace period), conflicts with R-FILE-004 resolved by "unreferenced only" |
| R-READ-007, R-READ-008 | half-page turns, conductor push | half-page turns kept as a page-view option (the system view covers the need on small tablets, see R-READ-007 notes); conductor push needs server push (SSE/WebSocket) — new component, ask user |
| R-READ-017 | split wide systems at a barline | needs barline detection; today such music is shown at full width (`below_min`) |
| R-ANN-017, R-ANN-018 | shared sets, encryption at rest | shared sets conflict with F-ANN-004 (explicit, read-only) |
| R-OFF-001, R-OFF-002, R-OFF-003 | encrypted offline app | depends on file hashes (d-blobs) and page revisions (d-optimistic); plan in requirement notes |
| R-ACC-011, R-ACC-012 | OIDC login, e-mail invites | SMTP stays optional |
| R-ACL-007 | several ensembles per server | open question; `ensemble_id` scoping every query, cheap early / expensive late |
| R-ANN-019, R-ANN-020 | shapes, JSON export | shapes need ink format version 2 |
| R-FILE-010 | bilevel compression of scans | |
| R-SEC-009, R-SEC-010 | watermark, download audit log | watermark must not create blobs (conflict with R-FILE-004) |
| R-OPS-009 | continuous replication (Litestream) | |
| F-OPS-005, R-OPS-010, R-OPS-012, R-OPS-018…022, R-OPS-025, R-OPS-038…041 | updates and downgrades (d-update-model, a-preview-migrations, a-patch-no-db-changes, a-db-check) | plan and roadmap in `docs/technical/chapters/14-updates.typ`; done: own update runner (a-update-runner; R-OPS-011, 013–017 mechanism, 023, 024), fallbacks for unknown stored values (a-stored-fallbacks). Still for 1.0.0: snapshot + automatic restore, `backup` without updating, `restore`, CI checks, release pipeline (R-OPS-049, PR G); see § Updates below |
| F-OPS-008, R-OPS-043…048 | open clients during shutdown, start and update (a-client-outage) | listener before updates with 503 `server_starting`, shutdown drain with 503 `server_shutting_down` (new `ErrorCode`s), banners, polling, unsaved markings kept in memory and re-sent, `beforeunload` warning, reload at a safe moment after a version change |
| F-OPS-006, R-OPS-026…032, R-OPS-042 | platforms (a-platforms) | multi-arch images + version tags (phase 0); Windows blocker and other findings in § Updates below; program archives + `SHA256SUMS` (unsigned; guide text for SmartScreen/Gatekeeper), embedded web client, OS data folder, platform-dependent CI (tests, build, smoke) on all systems, the rest Linux-only (phase 1) |
| F-OPS-007, R-OPS-036 | version visible (a-version-public) | done: `/api/version`, About view, title pages from the build's version (R-OPS-033…035); missing: the title-page check in stage 1 of the release pipeline (R-OPS-035, PR G); docs tags `vX.Y.Z` in requirements, technical docs and user guide by hand after each release (R-OPS-036, RELEASING.md § 4) |
| R-OPS-049 | release pipeline (a-release-pipeline) | PR G: dry run on the `release/<version>` pull request, real run on its merge; stage 1 builds and checks everything into expiring artifacts, stage 2 (only if all green) pushes images → tag → GitHub release with archives + `SHA256SUMS` → guide site, each step idempotent; `user-guide-pages.yml` becomes the last step (manual test build stays) |
| R-OPS-037 | backups/exports unencrypted, with guidance (a-unencrypted-export) | user guide backup section must say that backups need the same care as the data directory |
| F-TECH-002, R-TECH-004…007 | prepared replaceability | storage modules per data area (no SQL in `routes/`), file store without `path()`, PDF engine module, conformance suites; no second backend |

| F-LIB-004 phase 2 | mass import extensions | pattern per subfolder (archives mixing structures; today: several batches), ZIP upload, server-side paging of the review for very large batches, CSV inventory and OCR sources (R-LIB-022) |
| R-LIB-022 | OCR / inventory list as import sources | add candidates with `Source::Ocr` / `Source::Inventory` to `PieceGuess` fields (a-import-shared) |
| F-I18N-001 (R-I18N-001…010) | languages | phases (`docs/technical/chapters/13-i18n.typ`): 0 decisions/requirements/glossary done; 1 catalogue crate + `locale`; 2 errors as codes (separate branch); 3 part names in the client; 4 all client strings in the catalogue; 5 sorting in the browser; 6 German catalogue shipped as an agent draft (a-i18n-german; maintainer review open); 7 guide per language (R-I18N-009), Weblate later |

Also not implemented (no requirement yet): user deletion, rate limiting beyond login, metrics.

## Deviations from the original design (see `background.md` §7)

- Annotation set migration checks page count only, not page size, and does not flag moved sets for review (R-ANN-016 notes).
- No `getCoalescedEvents()` for full pen sampling rate (a-live-ink).
- Renderer abstraction is the `bridge.rs` function set, not a `PageRenderer` trait.
- Neighbouring pages (±2) are not pre-rendered in the page view; pages render on demand from in-memory bytes. The
  system view pre-renders the next view's crops.
- pdf.js legacy build instead of the modern build (d-pdf-rendering); page GET for unsaved pages returns 204 (d-204);
  unsaved strokes flushed with keepalive on tab close (d-reader-store) — additions, not in the original design.

## Verification gaps (implemented but not covered by automated tests)

- Session expiry/sliding (R-ACC-005), reinvite (R-ACC-010), security headers (R-SEC-005), backup command (R-OPS-005), graceful shutdown (R-OPS-008), TLS (R-OPS-006).
- Performance requirements R-PERF-001/003/005/006 verified only by prototype benchmarks in `prototypes/` and a manual RSS measurement (≈11 MB idle).
- System view: only synthetic PDFs in automated tests; real scores checked manually with `examples/layout_report.rs`
  (numbers in `layout.md`). Physical staff size depends on the device density (a-css-mm). Detection time on a
  Raspberry Pi not measured (R-PERF-008 estimate). R-READ-019 (layouts prefetched with a programme) not e2e-tested.
- Import UI without e2e coverage: drag and drop of a folder (only the folder input is driven), resume after an interrupted upload (API-tested), bulk edit, pattern change, existing-piece confirmation, skip of single files, "Use for …" with an existing instrument (all API-tested). Preset detection and the "listed once" rule for instruments are unit-tested (`import/mod.rs`, `parts.rs`, `review.rs`) and API-tested for flat numbered files; the e2e smoke covers only the folder preset. The presets were checked on synthetic archives only (real archives with other layouts may need an own pattern). Hashing speed of very large archives in the browser not measured.
- Client features without e2e coverage: eraser/stamps/text/undo (R-ANN-002, R-ANN-003), palm rejection (R-ANN-004), sets menu actions, migration banner, wake lock, fullscreen, programme next/previous navigation (R-READ-005); deleting pieces/parts/programmes/groups from the UI, user role changes and disable/enable (API-tested only).
- Sharp rendering (a-device-pixels): painted = backing verified only in Chromium (real `--force-device-scale-factor`);
  Firefox and Safari (no `devicePixelContentBox`) not measured; real browser zoom (Ctrl +) and a real second screen are
  only simulated with CDP device-metrics emulation (sizes checked, not the painted pixels); pinch zoom sharpness checked
  by backing size only. The layout editor renders at the current ratio but does not re-render on a ratio change.
- arm64 (Raspberry Pi) build not tested; Caddy HTTPS profile not tested end to end; release wasm with current wasm-opt not tested in CI.
- Rust CI (`rust.yml`) runs only on Linux x86_64; nothing is built or tested on Windows, macOS or arm64, and no image is built in CI (R-OPS-032).
- User-guide Pages workflow (a-guide-site): the script is tested locally (tags, test build, skipped and failing versions, HTTPS fetch); the GitHub-side parts (`gh release list`, Pages deploy, `github-pages` environment rules for tags) are only verified by a real run.

## Known debt / quirks

- i18n: English and German are shipped (`locales/en`, `locales/de`). **German is an agent draft that needs the maintainer's review** (terminology, tone, "du"; open choices in `glossary.md` § Open choices; review aid `scripts/i18n-review.py de`). The user guide and its screenshots are English only (phase 7; `screenshots.mjs` and `smoke.mjs` force `locale: en-US`). The German run (`e2e/german.mjs`) checks a selection of pages, not every text (completeness is checked by `tests/catalogue.rs`). The pseudo-locale run (`e2e/pseudo.mjs`) covers 14 pages without the reader with a file, the import review and the system editor (those are covered only by the lint). Button overflow with long translations is only reported, not failed.

- `annotation_sets` name uniqueness is case-sensitive (SQLite UNIQUE without NOCASE) while users/groups/instruments are case-insensitive.
- `LoginLimiter` is in-memory: resets on restart, per process.
- i18n (F-I18N-001): errors, import notes, pattern/ink errors and staged-file problems are codes rendered by the client
  (`errors.rs` with `errors.ftl`, a-error-codes). Some client texts are coarser than the codes (`correction_bounds_*`, `if_match_*`, the damaged-ink cases share one text).
- Pages whose PDF page count is unknown accept annotation pages 0..1999.
- Client uses `window.prompt`/`confirm` for dialogs (no custom modal).
- Tag entry: the input is moved between chips only with an empty input (←/→/Home/End) or by clicking a chip; clicking between two chips (not on one) puts it at the end. Keystrokes typed in the very frame after a move could go to the old focus (refocus waits for `requestAnimationFrame`).
- Keys stored before migration 0002 that were ambiguous ("B") or free text stay as they are; they are shown verbatim in names and as "Other: …" in the key list until a librarian picks a listed key (a-part-names).
- `part_fts` is rebuilt on start only when its row count differs from `parts`; a change of the name format needs an explicit rebuild.
- Immediate instrument save in the part editor sends the last saved label/voices/key; unsaved typing in those fields still needs `Save part` (a-assign-picker).
- Uploads in the guided flow run in a task of the `NewPartForm` component; leaving the page via the navigation bar while one runs abandons waiting for it (`Finish` is disabled meanwhile). The browser usually completes the request anyway.
- Assignment picker lists every candidate in the DOM (no virtualisation); fine for a few thousand entries. Score grants and the programme picker load all scores (`GET /api/scores`, full objects with parts).
- A tap outside an open picker only closes it (backdrop); the tapped control needs a second tap (a-assign-picker). Leaving the page with the browser's back button while a picker is open drops the ticks.
- `programmes.rs::futures_join` awaits sequentially (works only because prefetch is eager, a-eager-prefetch).
- Pinch zoom re-renders at up to `MAX_CANVAS_PIXELS` per canvas (crop cache ≤ 3 × that); while pinched the canvas
  position is not on whole device pixels (browser-controlled), so it is sharp but not strictly 1:1.
- Thumbnails are blobs without a `files` row; never served except via their PDF's hash.
- Import: `GET /api/import/batches/{id}` returns the whole review (no server paging); fine for a few thousand files, a very large batch should be split. `register_files` handles the whole list in one write transaction (one statement per new file, then `repropose`); 20 000 files per request. Files of a deleted or undone batch stay as blobs and `files` rows (no GC, R-FILE-009).

## Updates, downgrades and platforms (planned; `docs/technical/chapters/14-updates.typ`)

- No release, no tag, version 0.1.0. The own update runner (a-update-runner) replaced `sqlx::migrate!`: the
  development migrations `0001`–`0010` are the previews `migrations/preview/0001-…`–`0010-…`; development databases
  that applied them (or a beginning of them) are recognised once. **Open for the 1.0.0 release (manual, `RELEASING.md`):**
  after `scripts/release-migrations.py 1.0.0` simplify `migrations/1.0.0.sql` by hand to the final schema (the script's
  automatic simplification covers only safe folds), keep its `-- @previews:` line, and check it against the appended
  version. Open points of the runner:
  derived data after another program ran = only the part search index (system layouts not re-checked); a due removal
  runs only at a start; unparsable stored versions block removals; the program version stays 0.1.0 until the release sets it; nothing checks yet that a release build's line is in
  `releases.txt` (release check, a-db-check). Verified only on temporary databases with test programs
  (`tests/updates.rs`); the upgrade/downgrade matrix with real release binaries comes with the second release.
- `backup` applies pending updates before copying (`main.rs` → `open_state` → `Db::open`), contrary to R-OPS-021;
  `Db::open` has the slot for the snapshot between `inspect` and `apply`.
- No automatic snapshot, no `restore` command (manual copy in the user guide).
- Version display (phase 0) is done: `GET /api/version` (public, `no-store`), "About Musicus" (`/about`), docs title
  pages from `build.sh` (`--input version`; fallback workspace version, else "dev"). The client does not yet compare
  `api::version()` with its own `musicus_api::VERSION` (reload after an update, R-OPS-048). No release check fails on a
  title page whose version differs from the release (R-OPS-035); the guide site only warns on a tag ≠ workspace version.
- `compose.yaml` builds locally (`build: .`, `musicus:latest`); no published image (R-OPS-010, R-OPS-028).
- Requirements referencing the schema snapshot once PR E adds it (`schema.sql`): table-related requirements
  (R-LIB-002/003/004/015/016, R-PERF-005, R-I18N-006) then get it in `implemented_by`; update files may not be referenced
  (`validate.py`).
- No database check, `db-change` notice or release check in CI, no schema snapshot (`schema.sql`,
  `scripts/db-schema.sh`) — all come with phase 0 (a-db-check, R-OPS-038…040), not earlier.
- Patches writing new kinds of stored values (R-OPS-041) are not caught by any check; the fallbacks
  (a-stored-fallbacks) and the release checklist line (`RELEASING.md` step 7) exist, the test matrix with release
  test data is still missing (phase 2).
- Open: container registry and image name (decided when the release pipeline is built, PR G; Docker Hub and/or GHCR).
- Today the server listens only after migrations (`open_state` before `bind`) and closes the listener on the stop
  signal (R-OPS-008); the client shows generic errors when the server is away and reloads only on 401.
- Stored enums and formats have fallbacks for unknown values (a-stored-fallbacks, inventory in `data-model.md`
  § Stored values); verified only with rows written directly into the database (API tests), not with data of a real
  newer release. Unknown fields inside readable import overrides are dropped when a librarian edits the item.
- Portability findings (details and actions in the plan):
  1. Fixed: `blobs.rs::sync_dir` (`File::open(dir)?.sync_all()`, which fails on Windows ⇒ every upload failed) is
     `#[cfg(unix)]`; elsewhere a no-op (NTFS journals the rename; the order "rename durable before the database
     commit" is therefore not forced on Windows — accepted). Compile-checked for `x86_64-pc-windows-gnu`; never run
     on Windows (no Windows CI yet, R-OPS-032).
  2. SIGTERM handling is `#[cfg(unix)]` (`main.rs::shutdown_signal`); Ctrl+C works everywhere — fine.
  3. The plain program needs `--web-dir` with the client files (embedding accepted, R-OPS-030).
  4. Default data dir `data` is relative to the working directory (OS-usual folders accepted, R-OPS-031).
  5. CI only on Linux x86_64; Docker images only for the host architecture.
  6. Fixed: blob commit = "exists? else rename" (`blobs.rs::place`); a failed rename whose target exists meanwhile
     (concurrent upload of the same new file, or on Windows a rename onto an open blob) counts as stored (unit test
     `a_target_that_appeared_meanwhile_counts_as_stored`). Not tested on Windows.
  7. `backup` passes the target path to SQLite via `to_string_lossy` (non-UTF-8 paths would be altered).
  8. Raspberry Pi 3 (1 GB) supported by decision but never measured (R-PERF-001 names the Pi 4; no Pi measured at all).
  9. Building on Windows needs a C toolchain for `ring`/SQLite (assumption; to check in CI).
