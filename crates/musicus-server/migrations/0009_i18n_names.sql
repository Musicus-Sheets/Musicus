-- no-transaction
-- Internationalisation (a-client-part-names, a-data-as-entered):
-- 1. `annotation_sets.name` becomes nullable: NULL = the set the reader
--    created without asking, shown as "My annotations" in the UI language.
--    Sets named exactly "My annotations" (the former English default) become
--    NULL. Several NULLs would pass UNIQUE (user_id, part_id, name), so a
--    partial unique index keeps one unnamed set per user and part.
--    SQLite cannot drop NOT NULL in place: the table is rebuilt with foreign
--    keys off (otherwise dropping the old table would cascade into
--    annotation_pages), in one transaction.
-- 2. `part_fts` is emptied: the index now holds the components and key
--    spellings instead of the English name; `part_index::rebuild_if_needed`
--    rebuilds it on start.
PRAGMA foreign_keys = OFF;
BEGIN;
CREATE TABLE annotation_sets_new (
    id           INTEGER PRIMARY KEY,
    user_id      INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    part_id      INTEGER NOT NULL REFERENCES parts(id) ON DELETE CASCADE,
    file_id      INTEGER NOT NULL REFERENCES files(id),
    name         TEXT,
    programme_id INTEGER REFERENCES programmes(id) ON DELETE SET NULL,
    is_default   INTEGER NOT NULL DEFAULT 0,
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL,
    UNIQUE (user_id, part_id, name)
);
INSERT INTO annotation_sets_new (id, user_id, part_id, file_id, name, programme_id, is_default, created_at, updated_at)
    SELECT id, user_id, part_id, file_id, CASE WHEN name = 'My annotations' THEN NULL ELSE name END,
           programme_id, is_default, created_at, updated_at
    FROM annotation_sets;
DROP TABLE annotation_sets;
ALTER TABLE annotation_sets_new RENAME TO annotation_sets;
CREATE UNIQUE INDEX annotation_sets_unnamed ON annotation_sets (user_id, part_id) WHERE name IS NULL;
DELETE FROM part_fts;
COMMIT;
PRAGMA foreign_keys = ON;
