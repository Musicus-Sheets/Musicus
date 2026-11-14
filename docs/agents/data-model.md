# Data model (SQLite, `crates/musicus-server/migrations/0001_init.sql`)

Times = Unix seconds (`db::now()`); dates = `YYYY-MM-DD` text; booleans = INTEGER 0/1; JSON arrays as TEXT.
Migrations: applied at start by `sqlx::migrate!` (R-OPS-004). NEVER edit an applied migration (checksum mismatch aborts start-up); add `000N_<name>.sql`. Update this file in the same commit.

## Tables

| Table | Columns (PK first) | Constraints / notes | Requirements |
| --- | --- | --- | --- |
| `users` | `id`, `username` UNIQUE NOCASE, `display_name`, `password_hash` NULL until invite accepted, `role` CHECK member/librarian/admin, `disabled`, `created_at` | stored lowercased by `insert_user` | R-ACC-001, R-ACC-008 |
| `invites` | `token_hash` BLOB PK, `user_id` FK cascade, `expires_at` | WITHOUT ROWID; idx user | R-ACC-001, R-ACC-002 |
| `sessions` | `id`, `token_hash` UNIQUE, `user_id` FK cascade, `created_at`, `last_seen_at`, `expires_at`, `user_agent` (≤200 chars) | idx user | R-ACC-005, R-ACC-006 |
| `groups` | `id`, `name` UNIQUE NOCASE | | R-ACL-004 |
| `group_members` | PK (`user_id`, `group_id`) | WITHOUT ROWID; idx group; cascade both | R-ACL-001 |
| `instruments` | `id`, `name` UNIQUE NOCASE, `family` | | R-LIB-003 |
| `files` | `id`, `sha256` UNIQUE, `mime`, `size_bytes`, `page_count` NULL if unknown, `thumbnail_sha256` NULL, `created_at` | one row per blob (thumbnails are blobs without own row) | R-FILE-004 |
| `scores` | `id`, `title`, `subtitle`, `composer`, `arranger`, `genre`, `tags` JSON array TEXT default `'[]'`, `notes`, `created_at`, `updated_at` | | R-LIB-001 |
| `score_fts` | FTS5 external content (`content='scores'`), cols title, subtitle, composer, arranger, genre, tags; `tokenize='unicode61 remove_diacritics 2'` | kept in sync by triggers `scores_fts_insert/delete/update` | R-LIB-004 |
| `parts` | `id`, `score_id` FK cascade, `label`, `voice`, `transposition`, `notes`, `created_at` | idx score | R-LIB-002 |
| `part_instruments` | PK (`instrument_id`, `part_id`) | WITHOUT ROWID; idx part; instrument FK RESTRICT (in use ⇒ 409) | R-LIB-002, R-LIB-003 |
| `part_files` | PK (`part_id`, `version`), `file_id` FK, `uploaded_at` | WITHOUT ROWID; idx file; current = MAX(version) | R-FILE-002 |
| `group_instrument_grants` | PK (`group_id`, `instrument_id`) | WITHOUT ROWID; idx instrument | R-ACL-001 |
| `group_score_grants` | PK (`group_id`, `score_id`) | WITHOUT ROWID; idx score | R-ACL-001 |
| `programmes` | `id`, `title`, `date`, `notes`, `created_at` | | R-PROG-001 |
| `programme_items` | PK (`programme_id`, `position`), `score_id` FK cascade | WITHOUT ROWID; position = index in `IdList` | R-PROG-001 |
| `annotation_sets` | `id`, `user_id` FK cascade, `part_id` FK cascade, `file_id` FK (file version), `name`, `programme_id` FK SET NULL, `is_default`, `created_at`, `updated_at` | UNIQUE (`user_id`, `part_id`, `name`) — case-sensitive | R-ANN-008, R-ANN-016 |
| `annotation_pages` | PK (`set_id`, `page`), `revision`, `data` BLOB = zstd(ink bytes), `updated_at` | WITHOUT ROWID; page 0-based | R-ANN-012 |

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

## Deletion semantics

Delete score ⇒ parts ⇒ part_files, part_instruments, annotation_sets ⇒ annotation_pages (cascade); programme_items referencing the score cascade. `files` rows and blobs remain (INV-4). Delete user ⇒ not exposed via API (only disable). Delete group ⇒ memberships and grants cascade. Delete programme ⇒ items cascade, `annotation_sets.programme_id` set NULL.

## Data directory

`<data>/musicus.sqlite` (+ `-wal`, `-shm`), `<data>/blobs/ab/<sha256>`, `<data>/tmp/` (upload temp files, emptied on start). Backup order: `musicus-server backup` first, then copy `blobs/` (blobs only ever added ⇒ consistent).
