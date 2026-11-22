# Data model (SQLite; before 1.0: the previews `crates/musicus-server/migrations/preview/0001-init.sql` … `0010-german-key-spellings.sql`)

Times = Unix seconds (`db::now()`); dates = `YYYY-MM-DD` text; booleans = INTEGER 0/1; JSON arrays as TEXT.
Database updates: applied at start by the own update runner (`crates/musicus-server/src/updates/`, R-OPS-004, a-update-runner). A schema change is a new *preview* `migrations/preview/NNNN-<topic>.sql` with sections `-- @additive` / `-- @rebuild` / `-- @removal` (format in `updates/file.rs`); a merged preview or a released update is NEVER edited (correction = new preview); the release merges previews into `migrations/X.Y.0.sql` (`scripts/release-migrations.py`). Update this file in the same commit.
Before 1.0 there is no release update file: the development migrations 0001–0010 of the sqlx time are the previews 0001–0010 (text unchanged, inside `-- @additive`; destructive steps there are acceptable because development databases have no downgrade guarantee). Databases that applied sqlx migrations 1..k are recognised once as having the previews 0001..k. The release of 1.0.0 merges them into `migrations/1.0.0.sql` and the maintainer simplifies that by hand to the final schema (`RELEASING.md`). The table notes "(0003)", "(0006)" etc. name the preview that added a column.

## Tables

| Table | Columns (PK first) | Constraints / notes | Requirements |
| --- | --- | --- | --- |
| `users` | `id`, `username` UNIQUE NOCASE, `display_name`, `password_hash` NULL until invite accepted, `role` CHECK member/librarian/admin, `disabled`, `created_at`, `reader_mode` CHECK page/systems default page, `staff_size` CHECK S/M/L/XL default M (0003) | stored lowercased by `insert_user` | R-ACC-001, R-ACC-008, R-READ-014 |
| `invites` | `token_hash` BLOB PK, `user_id` FK cascade, `expires_at` | WITHOUT ROWID; idx user | R-ACC-001, R-ACC-002 |
| `sessions` | `id`, `token_hash` UNIQUE, `user_id` FK cascade, `created_at`, `last_seen_at`, `expires_at`, `user_agent` (≤200 chars) | idx user | R-ACC-005, R-ACC-006 |
| `groups` | `id`, `name` UNIQUE NOCASE | | R-ACL-004 |
| `group_members` | PK (`user_id`, `group_id`) | WITHOUT ROWID; idx group; cascade both | R-ACL-001 |
| `instruments` | `id`, `name` UNIQUE NOCASE, `family` | | R-LIB-003 |
| `files` | `id`, `sha256` UNIQUE, `mime`, `size_bytes`, `page_count` NULL if unknown, `thumbnail_sha256` NULL, `created_at` | one row per blob (thumbnails are blobs without own row) | R-FILE-004 |
| `scores` | `id`, `title`, `subtitle`, `composer`, `arranger`, `genre`, `tags` JSON array TEXT default `'[]'`, `notes`, `created_at`, `updated_at`, `archive_number` TEXT NULL (0006; free text ≤40, trimmed), `archive_search` TEXT NULL (0006; `naming::archive_search_text(archive_number)`, written by `library::insert_score`/`update_score`) | archive number NOT unique (a-archive-number); no index (import matching loads all numbers and compares `naming::archive_key`) | R-LIB-001, R-LIB-015 |
| `score_fts` | FTS5 external content (`content='scores'`), cols title, subtitle, composer, arranger, genre, tags, archive_search (0006); `tokenize='unicode61 remove_diacritics 2'` | kept in sync by triggers `scores_fts_insert/delete/update`; 0006 dropped and recreated table + triggers and ran `'rebuild'` | R-LIB-004, R-LIB-015 |
| `parts` | `id`, `score_id` FK cascade, `label` TEXT NULL = custom name (NULL ⇒ derived name, a-part-names), `transposition` TEXT NULL = key code of `musicus_api::naming::KEYS` (Bb, Eb, F, C, A, D, G, E, Ab, Db, Gb, H) or an older free-text value, `notes`, `created_at` | idx score; 0002 rebuilt `label`/`voice` with ADD/DROP/RENAME COLUMN; 0005 dropped `voice` (now `part_voices`); column order: id, score_id, transposition, notes, created_at, label | R-LIB-002, R-LIB-010, R-LIB-011 |
| `part_voices` | PK (`part_id`, `position` 0-based), `voice` TEXT NOT NULL COLLATE NOCASE (≤20 chars, checked by the server), UNIQUE (`part_id`, `voice`) | WITHOUT ROWID; part FK cascade; idx `voice` (for a later grant per voice, R-ACL-005 proposed; not used by `part_visible!()` yet); written by `library::set_voices` (delete + `json_each` insert, key = position); read as `json_group_array(json_array(position, voice))` and sorted in Rust (`library::voices_from_json`) (0005, a-part-voices) | R-LIB-002, R-LIB-010 |
| `part_fts` | FTS5 standalone, col `name` = `part_index::index_text` (components: custom name, instrument names, voices, key code + the key's names in every shipped language from `musicus_i18n::KEY_SPELLINGS`; no composed name, a-client-part-names; emptied by 0009 and 0010 (German key names) for a rebuild; every change of `KEY_SPELLINGS` needs a preview update `DELETE FROM part_fts;`, guarded by `part_index::tests::indexed_key_spellings_are_current`), rowid = part id; `unicode61 remove_diacritics 2` | written by the server (`part_index::reindex`) on part create/update and instrument rename; trigger `parts_fts_delete` removes rows; rebuilt on start if row count ≠ parts or another program used the database last | R-LIB-004, R-LIB-010 |
| `part_instruments` | PK (`instrument_id`, `part_id`) | WITHOUT ROWID; idx part; instrument FK RESTRICT (in use ⇒ 409) | R-LIB-002, R-LIB-003 |
| `file_layouts` | `file_id` PK FK files cascade, `algorithm` (detection version), `state` CHECK done/failed, `pages` TEXT = JSON `Vec<musicus_api::layout::PageLayout>` (`[]` if failed), `updated_at` | written only by `layout::worker` (upsert); no row = pending (0003) | R-READ-010 |
| `layout_corrections` | PK (`file_id`, `page`) 0-based, `data` TEXT = JSON `PageCorrection {full_page, bounds, kinds}`, `updated_by` FK users SET NULL, `updated_at` | WITHOUT ROWID; file FK cascade; applied on read, never touched by the worker (0004) | R-READ-016 |
| `part_files` | PK (`part_id`, `version`), `file_id` FK, `uploaded_at` | WITHOUT ROWID; idx file; current = MAX(version) | R-FILE-002 |
| `group_instrument_grants` | PK (`group_id`, `instrument_id`) | WITHOUT ROWID; idx instrument | R-ACL-001 |
| `group_score_grants` | PK (`group_id`, `score_id`) | WITHOUT ROWID; idx score | R-ACL-001 |
| `import_batches` | `id`, `name` (selected folder), `pattern`, `language` CHECK de/en, `created_by` FK users SET NULL, `created_at`, `updated_at` | (0007) | R-LIB-016, R-LIB-017 |
| `import_files` | `id`, `batch_id` FK cascade, `path` (below the folder), `size_bytes`, `sha256` (from the browser), `state` CHECK pending/uploaded/duplicate/failed, `file_id` FK files NULL, `problem` (JSON of an `ErrorCode`: `duplicate_in_library {title}`, `duplicate_in_batch {path}`, or the rejection code of a failed upload; migration 0008 renamed it from `message` and converted old texts) | UNIQUE (`batch_id`, `path`); idx sha256, file_id. A staged file = `files` row without `part_files` ⇒ invisible through `/api/files` (readable_file needs a part) | R-LIB-016 |
| `import_items` | `id`, `batch_id` FK cascade, `kind` CHECK piece/part, `piece_id` FK import_items cascade (parts), `file_id` FK import_files cascade (parts), `key` (piece: grouping key `n:…`/`t:…`; part: path), `position`, `proposal` JSON `PieceGuess`/`PartGuess`, `overrides` JSON `PieceOverrides`/`PartOverrides` default `'{}'`, `state` CHECK pending/committed, `score_id` FK scores SET NULL, `part_id` FK parts SET NULL, `created_score` 0/1, `committed_at` | idx (batch_id, kind), piece_id, file_id. Never resolved in SQL: the review resolves on read (a-import-shared) | R-LIB-017, R-LIB-019, R-LIB-021 |
| `instrument_aliases` | `alias` PK (folded words, `parts::alias_key`), `instrument_id` FK instruments CASCADE, `created_by` FK users SET NULL, `created_at` | WITHOUT ROWID | R-LIB-020 |
| `programmes` | `id`, `title`, `date`, `notes`, `created_at` | | R-PROG-001 |
| `programme_items` | PK (`programme_id`, `position`), `score_id` FK cascade | WITHOUT ROWID; position = index in `IdList` | R-PROG-001 |
| `annotation_sets` | `id`, `user_id` FK cascade, `part_id` FK cascade, `file_id` FK (file version), `name` TEXT NULL (NULL = the unnamed set the reader creates, shown as "My annotations" in the UI language), `programme_id` FK SET NULL, `is_default`, `created_at`, `updated_at` | UNIQUE (`user_id`, `part_id`, `name`) — case-sensitive; partial UNIQUE index `annotation_sets_unnamed` (`user_id`, `part_id`) WHERE name IS NULL (one unnamed set per part); 0009 rebuilt the table (foreign keys off, `-- no-transaction` + own BEGIN/COMMIT) and set rows named exactly "My annotations" to NULL | R-ANN-008, R-ANN-016 |
| `annotation_pages` | PK (`set_id`, `page`), `revision`, `data` BLOB = zstd(ink bytes), `updated_at` | WITHOUT ROWID; page 0-based | R-ANN-012 |

## Version marker (written by the update runner, not by an update file; `updates/runner.rs`, d-update-model)

| Table | Columns | Notes |
| --- | --- | --- |
| `musicus_meta` | `key` PK, `value` | WITHOUT ROWID. Keys: `marker_format` (`1`), `last_version` (program that last started), `last_updates` (JSON names it knew, incl. previews merged into its release updates), `oldest_allowed` (set by a removal; older programs refuse to start, R-OPS-015) |
| `musicus_updates` | `name` PK (`1.0.0` / `0003-topic`), `kind` release/preview, `release`, `additive_at`, `rebuilt_at`, `removal_at` (NULL: pending or none), `previews` JSON, `applied_by` (version, or `sqlx migrations 0001-0010` for a recognised development database) | WITHOUT ROWID; rows of unknown updates (newer release, other branch) are ignored |
| `musicus_upgrades` | `id`, `from_version`, `to_version`, `at` | every start of another version than `last_version`; rule (b) of the window |

Development databases of the `sqlx::migrate!` time keep their `_sqlx_migrations` table (unused).

## Access query shape (`access::part_visible!()`; needs alias `p` for parts)

```sql
(?2 = 1
 OR EXISTS (SELECT 1 FROM part_instruments acl_pi
            JOIN group_instrument_grants acl_g ON acl_g.instrument_id = acl_pi.instrument_id
            JOIN group_members acl_m ON acl_m.group_id = acl_g.group_id
            WHERE acl_pi.part_id = p.id AND acl_m.user_id = ?1)
 OR EXISTS (SELECT 1 FROM group_score_grants acl_s
            JOIN group_members acl_m2 ON acl_m2.group_id = acl_s.group_id
            WHERE acl_s.score_id = p.score_id AND acl_m2.user_id = ?1))
```
Bind `?1` = user id, `?2` = `access::librarian_flag(user)`; further params start at `?3`. Resolves via PK lookups (PK orders chosen for this) — R-PERF-005.

## Preview 0002 (part names)

Existing `label` ⇒ custom name (blank ⇒ NULL); numeric `voice` ⇒ text; `transposition`: blank ⇒ NULL, unambiguous spellings (case-insensitive, optional `in ` prefix: bb/b♭/b-flat/b flat/bflat, eb/…/es, ab/…/as, db/…/des, gb/…/ges, f, c, a, d, g, e, h/b♮/b natural) ⇒ code; everything else (e.g. `B`, ambiguous German/English, or `Es/F`) unchanged and kept on update. Mirrors `naming::parse_key`; test `migration_0002_keeps_old_parts` (R-LIB-012).

## Preview 0005 (voices)

`part_voices` created; each non-blank `parts.voice` becomes the only entry (position 0, trimmed, not split: "1+2" stays one voice; nothing was released, so no conversion); `parts.voice` dropped; `part_fts` emptied so the start-up rebuild re-indexes all parts. Test `migration_0002_keeps_old_parts` runs through it (voices `["2"]`, `[]`).

## Deletion semantics

Delete score ⇒ parts ⇒ part_files, part_instruments, part_voices, annotation_sets ⇒ annotation_pages (cascade); programme_items referencing the score cascade. `files` rows and blobs remain (INV-4). Delete user ⇒ not exposed via API (only disable). Delete group ⇒ memberships and grants cascade. Delete programme ⇒ items cascade, `annotation_sets.programme_id` set NULL. Delete import batch ⇒ its files and items cascade (pieces, parts, `files` rows and blobs stay). Delete instrument ⇒ its aliases cascade. Delete score/part ⇒ `import_items.score_id`/`part_id` set NULL (the item stays committed; undo skips it).

## Data directory

`<data>/musicus.sqlite` (+ `-wal`, `-shm`), `<data>/blobs/ab/<sha256>`, `<data>/tmp/` (upload temp files, emptied on start). Backup order: `musicus-server backup` first, then copy `blobs/` (blobs only ever added ⇒ consistent).

## Preview 0009 (names in the UI language, a-client-part-names)

Relies on the runner turning foreign keys off around the part (as sqlx time: its own `PRAGMA … BEGIN … COMMIT`, removed
in the preview), because SQLite cannot drop `NOT NULL` in place and dropping the old `annotation_sets` with foreign keys
on would cascade into `annotation_pages`. New table with nullable `name`, copy (exactly "My annotations" ⇒ NULL), drop,
rename, `annotation_sets_unnamed` partial unique index, `DELETE FROM part_fts` (rebuilt by
`part_index::rebuild_if_needed` with the component text). Test `migration_0009_unnames_default_sets_and_keeps_their_pages`.

## Preview 0010 (German key spellings, R-I18N-007)

`DELETE FROM part_fts` only: German ships, so `KEY_SPELLINGS` gains B, Es, As, Des, Ges (H is the code itself); the
index of existing installations is rebuilt on start by `part_index::rebuild_if_needed`.

