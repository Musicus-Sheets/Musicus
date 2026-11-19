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

| F-LIB-004 phase 2 | mass import extensions | pattern per subfolder (archives mixing structures; today: several batches), ZIP upload, server-side paging of the review for very large batches, CSV inventory and OCR sources (R-LIB-022), editing several instruments per part in the review (today one instrument via the select; more after the import in the part editor) |
| R-LIB-022 | OCR / inventory list as import sources | add candidates with `Source::Ocr` / `Source::Inventory` to `PieceGuess` fields (a-import-shared) |

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
- Import UI without e2e coverage: drag and drop of a folder (only the folder input is driven), resume after an interrupted upload (API-tested), bulk edit, pattern change, existing-piece confirmation, skip of single files, "Use for …" with an existing instrument (all API-tested). Hashing speed of very large archives in the browser not measured.
- Client features without e2e coverage: eraser/stamps/text/undo (R-ANN-002, R-ANN-003), palm rejection (R-ANN-004), sets menu actions, migration banner, wake lock, fullscreen, programme next/previous navigation (R-READ-005); deleting pieces/parts/programmes/groups from the UI, user role changes and disable/enable (API-tested only).
- Sharp rendering (a-device-pixels): painted = backing verified only in Chromium (real `--force-device-scale-factor`);
  Firefox and Safari (no `devicePixelContentBox`) not measured; real browser zoom (Ctrl +) and a real second screen are
  only simulated with CDP device-metrics emulation (sizes checked, not the painted pixels); pinch zoom sharpness checked
  by backing size only. The layout editor renders at the current ratio but does not re-render on a ratio change.
- arm64 (Raspberry Pi) build not tested; Caddy HTTPS profile not tested end to end; release wasm with current wasm-opt not tested in CI.
- No CI for Rust build/tests yet (only the docs gates `agent-docs.yml`, `submodules.yml` and the user-guide Pages deploy `user-guide-pages.yml`).
- User-guide Pages workflow (a-guide-site): the script is tested locally (tags, test build, skipped and failing versions, HTTPS fetch); the GitHub-side parts (`gh release list`, Pages deploy, `github-pages` environment rules for tags) are only verified by a real run.

## Known debt / quirks

- `annotation_sets` name uniqueness is case-sensitive (SQLite UNIQUE without NOCASE) while users/groups/instruments are case-insensitive.
- `LoginLimiter` is in-memory: resets on restart, per process.
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
