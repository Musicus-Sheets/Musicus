# Status

Source of truth for requirement status: `docs/requirements` (`status` field; overview in `docs/requirements/index.md`). This file adds what the requirements do not say: gaps, debt, verification limits.

## Implemented (version 0.1.0)

Everything in `server.md` endpoint table and `web-client.md`; requirements with status `implemented`/`verified`.

## Not implemented (requirements with status `proposed`)

| Requirement(s) | Topic | Notes for implementers |
| --- | --- | --- |
| R-ACL-005, R-ACL-006 | grants per voice / per programme / expiry | extend `part_visible!()` (d-access); new tables + migration |
| R-LIB-007 | extended score metadata | migration + `ScoreInput`/`Score` + manage form + FTS columns if searchable |
| R-FILE-008 | MusicXML | new processor in `processors.rs`; `sniff_mime`; annotations need musical anchors (not page coords) — design first |
| R-FILE-009 | GC of unreferenced blobs | must respect backup consistency (grace period), conflicts with R-FILE-004 resolved by "unreferenced only" |
| R-READ-007, R-READ-008 | half-page turns, conductor push | conductor push needs server push (SSE/WebSocket) — new component, ask user |
| R-ANN-017, R-ANN-018 | shared sets, encryption at rest | shared sets conflict with F-ANN-004 (explicit, read-only) |
| R-OFF-001, R-OFF-002, R-OFF-003 | encrypted offline app | depends on file hashes (d-blobs) and page revisions (d-optimistic); plan in requirement notes |
| R-ACC-011, R-ACC-012 | OIDC login, e-mail invites | SMTP stays optional |
| R-ACL-007 | several ensembles per server | open question; `ensemble_id` scoping every query, cheap early / expensive late |
| R-READ-009 | swipe page turns | |
| R-ANN-019, R-ANN-020 | shapes, JSON export | shapes need ink format version 2 |
| R-FILE-010 | bilevel compression of scans | |
| R-SEC-009, R-SEC-010 | watermark, download audit log | watermark must not create blobs (conflict with R-FILE-004) |
| R-OPS-009 | continuous replication (Litestream) | |

Also not implemented (no requirement yet): user deletion, rate limiting beyond login, metrics.

## Deviations from the original design (see `background.md` §7)

- Annotation set migration checks page count only, not page size, and does not flag moved sets for review (R-ANN-016 notes).
- No `getCoalescedEvents()` for full pen sampling rate (a-live-ink).
- Renderer abstraction is the `bridge.rs` function set, not a `PageRenderer` trait.
- Neighbouring pages (±2) are not pre-rendered; pages render on demand from in-memory bytes.
- pdf.js legacy build instead of the modern build (d-pdf-rendering); page GET for unsaved pages returns 204 (d-204);
  unsaved strokes flushed with keepalive on tab close (d-reader-store) — additions, not in the original design.

## Verification gaps (implemented but not covered by automated tests)

- Session expiry/sliding (R-ACC-005), reinvite (R-ACC-010), instrument deletion conflict (R-LIB-003), security headers (R-SEC-005), backup command (R-OPS-005), graceful shutdown (R-OPS-008), TLS (R-OPS-006).
- Performance requirements R-PERF-001/003/005/006 verified only by prototype benchmarks in `prototypes/` and a manual RSS measurement (≈11 MB idle).
- Client features without e2e coverage: eraser/stamps/text/undo (R-ANN-002, R-ANN-003), palm rejection (R-ANN-004), sets menu actions, migration banner, wake lock, fullscreen, programme next/previous navigation (R-READ-005), manage pages beyond the smoke flow.
- arm64 (Raspberry Pi) build not tested; Caddy HTTPS profile not tested end to end; release wasm with current wasm-opt not tested in CI.
- No CI for Rust build/tests yet (only `.github/workflows/agent-docs.yml`).

## Known debt / quirks

- `annotation_sets` name uniqueness is case-sensitive (SQLite UNIQUE without NOCASE) while users/groups/instruments are case-insensitive.
- `LoginLimiter` is in-memory: resets on restart, per process.
- Pages whose PDF page count is unknown accept annotation pages 0..1999.
- Client uses `window.prompt`/`confirm` for dialogs (no custom modal).
- `programmes.rs::futures_join` awaits sequentially (works only because prefetch is eager, a-eager-prefetch).
- Thumbnails are blobs without a `files` row; never served except via their PDF's hash.
