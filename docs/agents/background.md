# Background: brief, assumptions, alternatives, measurements, open questions

The complete design rationale of Musicus. Everything from the design phase that is still valid is here or in
`docs/requirements`. If the code deviates from what is written here, the code and the other files in `docs/agents`
describe the current state; update this file when a deviation is intended to stay.

## 1. Brief (original user requirements)

1. Server-side library of sheet files: PDF now, MusicXML and other formats later. → G-001, G-007, F-FILE-001, F-FILE-003
2. User management with authorisation; groups (e.g. "Trumpets") get access through metadata (instrument), not per file. → G-002, F-ACL-001
3. Members draw annotations; never modify the file; private; saved on the server; several named sets per user (e.g. per concert). → G-003, F-ANN-001..004
4. No persistent client data for now; later an app with encrypted storage and offline mode. → F-SEC-001, F-OFF-001
5. Backend in Rust, GUI in Dioxus. → F-TECH-001
6. Many musicians load the same file at the same moment (a piece starts for everyone). → F-PERF-002
7. Server must run on relatively small systems. → F-PERF-001
8. User decisions taken later: SQLite instead of PostgreSQL for now ("not aiming for scaling; PostgreSQL stays an option"); Docker/compose packaging wanted.

## 2. Assumptions (where the brief is open)

- One ensemble of 20–120 musicians per server (multi-ensemble: R-ACL-007, proposed).
- Library of a few thousand scores; tested up to 100 000 scores / 2 000 000 parts.
- Parts 0.1–3 MB (engraved exports) or 2–10 MB (scans; 0.1 MB/page bilevel to 1 MB/page colour JPEG).
- Tablets are the main reading devices; Bluetooth page-turn pedals send arrow/page keys.
- Typical annotation activity: 120 musicians drawing at once ≈ 60 page saves/s (one save per 2 s of drawing).
- Long-term data: ~5 years of a 120-member orchestra annotating 200 parts/year in 2 sets ≈ 1 000 000 pages of ~6 KB.

## 3. The burst is a network problem (why programmes are prefetched)

Transfer time when everyone downloads the same piece at once, at the effective shared throughput of one access point
(typical values, not measured in a specific room):

| Scenario | Data | 50 Mbit/s (busy 2.4 GHz) | 200 Mbit/s (good Wi-Fi 5) | 600 Mbit/s (Wi-Fi 6, ideal) |
| --- | --- | --- | --- | --- |
| 30 × 1 MB engraved | 30 MB | 4.8 s | 1.2 s | 0.4 s |
| 60 × 3 MB | 180 MB | 29 s | 7.2 s | 2.4 s |
| 100 × 10 MB scanned | 1 GB | 160 s | 40 s | 13 s |

The prototype server needed 0.2 s of CPU for the middle row on one core (loopback). Consequence: download the whole
concert programme ahead of time (R-PROG-003, R-PERF-007), keep it in memory (R-SEC-001), serve hot files from RAM
(R-PERF-004). Further mitigations: bilevel compression of scans (R-FILE-010, proposed), recommend 5 GHz / Wi-Fi 6
access points (deployment advice).

## 4. Architectures considered (decision: A, later confirmed with SQLite by the user)

| Concept | Stack | Processes | RAM idle→burst | Offline-app readiness | Build effort | Ops effort | Ceiling |
| --- | --- | --- | --- | --- | --- | --- | --- |
| **A – lean monolith (chosen)** | Axum, SQLite WAL, file store, Dioxus web, REST | 1 | 25–50 MB measured in prototype; 11 MB idle measured in 0.1 | good (revisions, hashes) | low | very low | ~1000s of users, 1 node |
| B – classic services | Axum, PostgreSQL, S3-compatible store, presigned URLs | 3+ | ~300–600 MB (est.) | good | medium | medium–high | horizontal |
| C – Dioxus fullstack | server functions + SSR, SQLite, files | 1 | ~40–80 MB (est.) | weak (no stable public API) | low at first | low | as A |
| D – embedded key-value | Axum, redb/fjall, files | 1 | ~20–40 MB (est.) | good | medium–high | very low | as A |
| E – local-first sync | sync relay, SQLite, CRDT (Loro/Automerge), encrypted client replicas | 1 + client stores | ~40–80 MB server (est.) | native | high | low | as A |

Why not the others (keep these reasons when someone proposes them again):
- B: three services, ~10× RAM, too heavy for a Pi, more moving parts for volunteers; presigned URLs stay valid until
  expiry, which complicates immediate revocation (conflicts with R-ACL-003). Self-hosted object stores: MinIO community
  edition is source-only and no longer maintained (checked Oct 2026); Garage or SeaweedFS would be the choice.
- C: couples the API to Dioxus releases (pre-1.0, breaking changes between minors); server functions are not a
  versioned public API for the future app/scripts; Range/caching needs plain routes anyway; SSR useless behind a login.
  → d-shared-types.
- D: every index, join, migration and the access rule become hand-written application code; no ad-hoc queries/admin
  tools; search would need tantivy. SQLite is already small.
- E: highest complexity (key management, sync protocol, CRDT history growth for ink, offline revocation); contradicts
  "no persistent client data" now. A keeps the path open: file hashes (d-blobs) and page revisions (d-optimistic) are
  the sync primitives (R-OFF-002).

Benefits of A that the design relies on: one file + one folder to deploy and back up; fits a Pi; SQL joins give the
access rule for free; content addressing makes caching and offline sync simple; WAL readers never block.
Accepted disadvantages of A: single node without HA (mitigations: prefetched pieces stay usable, printed backup,
R-OPS-009 replication proposed); one writer at a time (bulk imports serialise; keep transactions short).

## 5. PostgreSQL variant "A-PG" (evaluated, not chosen for now)

Same architecture, PostgreSQL 16 in a second container (compose), files still on a volume. Measured with PostgreSQL
pinned to one core, `shared_buffers = 128MB`, sqlx pool, 200 000-part library:

| Area | SQLite (chosen) | PostgreSQL |
| --- | --- | --- |
| Containers | 1 | 2 (+ volumes pgdata, blobs) |
| Access check, one request | 24 µs in-process | 0.22 ms over socket |
| 120 simultaneous checks + file lookup | part of the 0.2 s burst | 26–41 ms |
| 120 simultaneous annotation saves | 22 ms | 38 ms (pool 4), 90 ms (pool 10) |
| DB RAM | inside the server process | +28 MB idle, +67–78 MB under load |
| DB size, 200 000 parts | 47 MB | 109 MB (+WAL, data dir 356 MB) |
| Backup | one file (`VACUUM INTO`, Litestream) | `pg_dump` or pgBackRest/WAL archiving |
| Major upgrades | file format stable | dump/restore or `pg_upgrade` (16→17) |
| Writers | one at a time | parallel (MVCC) |
| Tests | in-memory DB | needs a running PostgreSQL |

Large-library test (5 000 users, 100 000 scores, 2 000 000 parts, 1 000 000 annotation pages of 6 KB; server + DB on 2 cores):

| Measure | SQLite | PostgreSQL 16 |
| --- | --- | --- |
| Size on disk | 9.3 GB | 9.5 GB + 1.1 GB WAL |
| Load one annotation page | 43 µs | 290 µs |
| List a user's sets for a part | 9 µs | 172 µs |
| Access check, 2 M parts | 37 µs | 366 µs |
| FTS, 2 words, 100 000 scores | 0.7 ms (FTS5) | 1.3 ms (GIN tsvector) |
| Full online backup | 46 s (`VACUUM INTO`) | 45 s (`pg_dump`) |

Mixed load (4 writers non-stop + N readers non-stop, 2 runs × 20 s):

| Readers | SQLite reads/s | SQLite saves/s (p99) | PostgreSQL reads/s | PostgreSQL saves/s (p99) |
| --- | --- | --- | --- | --- |
| 0 | – | 2 500–2 800 (22–31 ms) | – | 3 200–3 500 (3–4 ms) |
| 2 | 7 200–11 300 | 1 800–1 900 (16–17 ms) | 3 500–4 000 | 2 400–2 800 (4 ms) |
| 8 | 21 400–21 800 | 560–670 (63–85 ms) | 8 600–9 000 | 1 700–1 800 (6 ms) |

Conclusions: data volume is no reason to switch (B-tree lookups stay µs; PostgreSQL is slower per query only because
of the socket). PostgreSQL scales better for: several server instances on one DB; many concurrent writers with low
tail latency (many ensembles); online schema changes on big tables (SQLite rebuilds the table except for ADD COLUMN,
minutes at 9 GB, blocks writes; VACUUM needs 2× space); built-in replication/PITR. Musicus' expected 60 saves/s are
~10× below SQLite's worst case. Both stored 6 KB ink blobs with ~50 % overhead (SQLite 4 KB overflow pages, PostgreSQL
2 KB TOAST chunks); a larger SQLite `page_size` would reduce it.

A-PG advantages (reasons that would justify switching later): Docker-only install anyway; better search (`unaccent`,
`pg_trgm` typo tolerance); `jsonb`/`citext`/`timestamptz`; parallel writers; `LISTEN/NOTIFY` for a conductor push
across instances (R-READ-008); mature admin tooling; shorter path to B.
A-PG disadvantages: +~70 MB DB RAM and ~60–100 MB for Docker (fits a 2 GB Pi, tight on 1 GB VPS); major-version
upgrades are the main self-hosting trap (pin the major version, document upgrades); backups across two volumes (dump
first, then files; delete unreferenced files only after a grace period); dev/CI need PostgreSQL (`sqlx` and `rusqlite`
cannot share a crate — both link native sqlite3); the future app uses SQLite locally (two dialects; acceptable);
constant WAL writes → SSD instead of SD card on a Pi.
If switching: PostgreSQL only (do not support both: doubles the test matrix); migrations at start; `bigint` identity,
`timestamptz`, `citext`, `bytea`; pool 4–8, `max_connections=30`, `shared_buffers=128MB`; compose with health check,
`depends_on: service_healthy`, password as Docker secret, no published DB port (sketch:
`prototypes/pg-proto/docker-compose.example.yml`); amd64+arm64 images; optional backup container (nightly `pg_dump` +
restic of the file volume). This is a user decision (d-monolith, d-sqlite).

## 6. Prototype measurements (`prototypes/`, synthetic data)

Setup: release builds, 4-core x86-64 container, server pinned to 1 core / 1 Tokio worker, load generator on other
cores, loopback, one TCP connection per client, barrier start, every request = session lookup + SQL access check.

| Test | Load | p50 | Max | Server RAM peak |
| --- | --- | --- | --- | --- |
| Download 3 MB from RAM | 20 / 60 / 120 clients | 26 / 78 / 116 ms | 43 / 143 / 199 ms | 26–27 MB |
| Download 3 MB from disk (page cache) | 120 | 163 ms | 263 ms | 25 MB |
| From RAM, 2 cores / 2 workers | 120 | 118 ms | 156 ms | 48 MB |
| Revalidate with ETag (304) | 120 | 6 ms | 10 ms | 28 MB |
| Save 8 KB annotation page | 120 | 18 ms | 22 ms | 29 MB |

Access check 18 µs (7 500 parts) / 24 µs (200 000 parts, 47 MB DB); library listing 2.4 ms for ~10 900 visible parts;
Argon2id verify 21 ms per login on one core (60 parallel logins unbounded would need 1.1 GB; with semaphore the 60th
waits 1.3 s on 1 core, 0.3 s on 4); hayro render engraved A4 10/15 ms, scanned 36/57 ms (108/180 dpi); hayro as WASM
3.7/4.5 MB raw, 1.1/1.7 MB gzip (without/with bundled fonts); pdf.js ~0.4–0.6 MB gzip (approximate). Ink sizes per
page — light 8×40 points: JSON 11.0 KB, JSON+zstd 4.2 KB, binary delta+zstd 1.3 KB; typical 30×60: 61.0 / 23.7 / 7.0 KB;
heavy 120×80: 322 / 130 / 36 KB. Raspberry Pi 4 cores are ~3–4× slower (estimate) → 120-client burst still < ~1 s CPU.
Caveats: loopback hides the network; synthetic engraved PDF (37 KB / 4 pages, no music font) and simulated scan (2.5 MB / 4 pages).

## 7. Design details beyond the requirements (still valid)

- Access rules are additive only — no deny rules — so access is always explainable ("you see it because you are in
  Trumpets and Trumpets has Trumpet"). d-access
- Roles are global and few (admin, librarian, member); access is never granted to a single user, only to groups.
- Coordinates 0–65 535 ≈ 3 µm resolution on A4. Eraser is an object eraser; a pixel eraser would split strokes and
  multiply objects (rejected for now).
- Annotations in the DB are readable by whoever has the DB file → disk encryption (e.g. LUKS) now, per-user encryption
  later (R-ANN-018); the payload is already an opaque blob, so no schema change is needed for that.
- Live pen input: Dioxus' pointer event wrapper does not expose `getCoalescedEvents()`; full pen sampling rate would
  need it via web-sys (not done; a-live-ink). Validate pen latency on a real tablet.
- Renderer abstraction: the original design named a `PageRenderer` interface; in 0.1 the abstraction is the set of
  functions in `crates/musicus-web/src/bridge.rs`. Re-evaluate hayro in the client once its coverage of real files is
  proven (d-pdf-rendering). Dioxus desktop/mobile run in a webview, so pdf.js also works in a future app; hayro is only
  needed for a webview-free renderer.
- Rendering neighbouring pages (±2) ahead was planned; 0.1 renders on demand from in-memory bytes (fast enough in tests).
- Open decision "HTTP cache for PDFs?" resolved as configurable, default `no-store` (R-FILE-007).
- TLS: browsers need a trusted certificate for `Secure` cookies, wake lock, service workers and Web Crypto. For a Pi in
  a rehearsal room without internet: real domain + DNS-01 Let's Encrypt certificate pointing to the local IP; or host
  on a VPS. Self-signed certificates break these features. (R-OPS-006)
- Backups: `VACUUM INTO` or Litestream (R-OPS-009) + restic/rsync of `blobs/` (immutable → incremental is cheap).
- Format-agnostic blob store; format-specific work in processors (validate, count pages, thumbnail) — MusicXML plan in
  F-FILE-003 notes; offline app plan in R-OFF-001/002/003 notes.
- Keep business logic and API types in plain Rust crates and the UI layer thin (Dioxus pre-1.0 risk); pin versions.

## 8. Limitations and risks

| Risk | Impact | Mitigation |
| --- | --- | --- |
| Single node, no HA | server down = no new sheets during a concert | prefetched pieces stay usable; printed backup; R-OPS-009 |
| Wi-Fi is the bottleneck | slow first load of large/scanned parts | programme prefetch; R-FILE-010; 5 GHz / Wi-Fi 6 |
| SQLite single writer | bulk imports briefly block saves | short transactions; PostgreSQL (§5) beyond ~1000s of active users |
| Dioxus pre-1.0 | breaking changes, small ecosystem | thin UI layer, shared plain crates, pinned versions |
| pdf.js = JavaScript in a Rust client | mixed code base | confined to `bridge.js` behind `bridge.rs` |
| Annotations tied to a file version | corrected upload does not move markings | old versions kept; guided migration (R-ANN-016) |
| DB file exposes annotations | privacy depends on server admin | disk encryption; R-ANN-018 |
| Copying cannot be prevented | screenshots, saved PDFs | R-SEC-009 watermark, R-SEC-010 audit log (deterrents) |
| Licensing of digital sheet music | scans may need publisher licences (in Germany e.g. via VG Musikedition) | rights-status metadata (R-LIB-007); legal check per ensemble |
| TLS on an offline Pi | self-signed breaks cookies/wake lock | DNS-01 certificate or VPS |
| Strictly private annotations | sections cannot share bowings/cuts | R-ANN-017 shared read-only sets |
| Pen latency through Dioxus events | lag at high sampling rates | imperative live drawing (a-live-ink); test on real tablets |

## 9. Open questions (ask the user before deciding)

- Deployment target: Pi in the rehearsal room (local Wi-Fi, DNS-01 TLS) or a VPS (works at home and at venues)?
- One ensemble per server or several (R-ACL-007)?
- Typical files: engraved exports or scans, and how large (R-FILE-010)?
- Are digital copies covered by the ensemble's agreements with publishers?
- Conductor push (R-READ-008) and shared section sets (R-ANN-017): wanted? (recorded as proposed)

## 10. External facts used (checked October 2026)

- Argon2id minimum m=19 MiB, t=2, p=1: OWASP Password Storage Cheat Sheet, https://github.com/OWASP/CheatSheetSeries/blob/master/cheatsheets/Password_Storage_Cheat_Sheet.md
- MinIO community edition source-only, not maintained: https://github.com/minio/minio
- OpenSheetMusicDisplay BSD-3-Clause: https://github.com/opensheetmusicdisplay/opensheetmusicdisplay
- Verovio LGPL-3.0, JavaScript toolkit: https://github.com/rism-digital/verovio
- hayro 0.8.0: pure-Rust PDF rasteriser, no performance work yet, no encrypted PDFs: https://crates.io/crates/hayro
- Dioxus 0.7.10 stable, 0.8 in alpha: https://crates.io/crates/dioxus
