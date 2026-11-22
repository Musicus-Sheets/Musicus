-- Development migration 0006 of the sqlx::migrate! time, as a preview (a-update-runner).
-- Text unchanged. Destructive steps in the additive part are acceptable before 1.0:
-- there is no downgrade guarantee for development databases.
-- @additive
-- Archive number of a piece (R-LIB-015, a-archive-number): free text such as
-- "0304" or "A-12", the number of the folder in the paper archive. Not
-- unique: archives contain gaps, typos and double numbers; the import uses it
-- only to suggest a match.
ALTER TABLE scores ADD COLUMN archive_number TEXT;
-- What the search indexes for it ("0304 304", "A-12 A12"), written by the
-- server (musicus_api::naming::archive_search_text) whenever the number changes.
ALTER TABLE scores ADD COLUMN archive_search TEXT;

-- The search index gets the new column: FTS5 tables cannot be altered, so the
-- index and its triggers are created again and rebuilt from `scores`.
DROP TRIGGER scores_fts_insert;
DROP TRIGGER scores_fts_delete;
DROP TRIGGER scores_fts_update;
DROP TABLE score_fts;
CREATE VIRTUAL TABLE score_fts USING fts5(
    title, subtitle, composer, arranger, genre, tags, archive_search,
    content = 'scores', content_rowid = 'id',
    tokenize = 'unicode61 remove_diacritics 2'
);
CREATE TRIGGER scores_fts_insert AFTER INSERT ON scores BEGIN
    INSERT INTO score_fts (rowid, title, subtitle, composer, arranger, genre, tags, archive_search)
    VALUES (new.id, new.title, new.subtitle, new.composer, new.arranger, new.genre, new.tags, new.archive_search);
END;
CREATE TRIGGER scores_fts_delete AFTER DELETE ON scores BEGIN
    INSERT INTO score_fts (score_fts, rowid, title, subtitle, composer, arranger, genre, tags, archive_search)
    VALUES ('delete', old.id, old.title, old.subtitle, old.composer, old.arranger, old.genre, old.tags, old.archive_search);
END;
CREATE TRIGGER scores_fts_update AFTER UPDATE ON scores BEGIN
    INSERT INTO score_fts (score_fts, rowid, title, subtitle, composer, arranger, genre, tags, archive_search)
    VALUES ('delete', old.id, old.title, old.subtitle, old.composer, old.arranger, old.genre, old.tags, old.archive_search);
    INSERT INTO score_fts (rowid, title, subtitle, composer, arranger, genre, tags, archive_search)
    VALUES (new.id, new.title, new.subtitle, new.composer, new.arranger, new.genre, new.tags, new.archive_search);
END;
INSERT INTO score_fts (score_fts) VALUES ('rebuild');
