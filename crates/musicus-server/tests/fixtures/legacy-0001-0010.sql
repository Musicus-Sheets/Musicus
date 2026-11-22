-- Test fixture: a development database of the sqlx::migrate! time, i.e. the
-- migrations 0001-0010 as they were applied (verbatim) plus sqlx's bookkeeping
-- table. Used by tests/updates.rs to check that such databases are recognised
-- as having the 1.0.0 baseline and that the baseline gives the same schema.
-- Never applied by the program; do not edit.

-- ===== 0001_init.sql
-- Musicus schema, version 1.
-- Times are Unix seconds. Sheet files live in the blob store on disk; this
-- database only references them by SHA-256.

CREATE TABLE users (
    id            INTEGER PRIMARY KEY,
    username      TEXT NOT NULL UNIQUE COLLATE NOCASE,
    display_name  TEXT NOT NULL,
    -- NULL until the user has accepted their invite.
    password_hash TEXT,
    role          TEXT NOT NULL CHECK (role IN ('member', 'librarian', 'admin')),
    disabled      INTEGER NOT NULL DEFAULT 0,
    created_at    INTEGER NOT NULL
);

-- One-time links to set a password (first login or reset).
CREATE TABLE invites (
    token_hash BLOB PRIMARY KEY,
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    expires_at INTEGER NOT NULL
) WITHOUT ROWID;
CREATE INDEX invites_user ON invites(user_id);

-- Opaque session tokens; only their SHA-256 is stored.
CREATE TABLE sessions (
    id           INTEGER PRIMARY KEY,
    token_hash   BLOB NOT NULL UNIQUE,
    user_id      INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at   INTEGER NOT NULL,
    last_seen_at INTEGER NOT NULL,
    expires_at   INTEGER NOT NULL,
    user_agent   TEXT
);
CREATE INDEX sessions_user ON sessions(user_id);

CREATE TABLE groups (
    id   INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE COLLATE NOCASE
);

CREATE TABLE group_members (
    group_id INTEGER NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    user_id  INTEGER NOT NULL REFERENCES users(id)  ON DELETE CASCADE,
    PRIMARY KEY (user_id, group_id)
) WITHOUT ROWID;
CREATE INDEX group_members_group ON group_members(group_id);

-- Controlled vocabulary, so "Trp.", "Trumpet" and "Trompete" cannot drift apart.
CREATE TABLE instruments (
    id     INTEGER PRIMARY KEY,
    name   TEXT NOT NULL UNIQUE COLLATE NOCASE,
    family TEXT
);

-- Immutable, content-addressed blobs (blobs/ab/abcdef...).
CREATE TABLE files (
    id               INTEGER PRIMARY KEY,
    sha256           TEXT NOT NULL UNIQUE,
    mime             TEXT NOT NULL,
    size_bytes       INTEGER NOT NULL,
    page_count       INTEGER,
    thumbnail_sha256 TEXT,
    created_at       INTEGER NOT NULL
);

-- A score is the work or arrangement.
CREATE TABLE scores (
    id         INTEGER PRIMARY KEY,
    title      TEXT NOT NULL,
    subtitle   TEXT,
    composer   TEXT,
    arranger   TEXT,
    genre      TEXT,
    -- JSON array of strings.
    tags       TEXT NOT NULL DEFAULT '[]',
    notes      TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

-- Full-text search over the score metadata. remove_diacritics makes
-- "Fucik" find "Fučík".
CREATE VIRTUAL TABLE score_fts USING fts5(
    title, subtitle, composer, arranger, genre, tags,
    content = 'scores', content_rowid = 'id',
    tokenize = 'unicode61 remove_diacritics 2'
);
CREATE TRIGGER scores_fts_insert AFTER INSERT ON scores BEGIN
    INSERT INTO score_fts (rowid, title, subtitle, composer, arranger, genre, tags)
    VALUES (new.id, new.title, new.subtitle, new.composer, new.arranger, new.genre, new.tags);
END;
CREATE TRIGGER scores_fts_delete AFTER DELETE ON scores BEGIN
    INSERT INTO score_fts (score_fts, rowid, title, subtitle, composer, arranger, genre, tags)
    VALUES ('delete', old.id, old.title, old.subtitle, old.composer, old.arranger, old.genre, old.tags);
END;
CREATE TRIGGER scores_fts_update AFTER UPDATE ON scores BEGIN
    INSERT INTO score_fts (score_fts, rowid, title, subtitle, composer, arranger, genre, tags)
    VALUES ('delete', old.id, old.title, old.subtitle, old.composer, old.arranger, old.genre, old.tags);
    INSERT INTO score_fts (rowid, title, subtitle, composer, arranger, genre, tags)
    VALUES (new.id, new.title, new.subtitle, new.composer, new.arranger, new.genre, new.tags);
END;

-- A part is one sheet of a score, e.g. "2nd Trumpet in Bb".
CREATE TABLE parts (
    id            INTEGER PRIMARY KEY,
    score_id      INTEGER NOT NULL REFERENCES scores(id) ON DELETE CASCADE,
    label         TEXT NOT NULL,
    voice         INTEGER,
    transposition TEXT,
    notes         TEXT,
    created_at    INTEGER NOT NULL
);
CREATE INDEX parts_score ON parts(score_id);

-- A part can serve several instruments ("Flute / Piccolo").
CREATE TABLE part_instruments (
    part_id       INTEGER NOT NULL REFERENCES parts(id) ON DELETE CASCADE,
    instrument_id INTEGER NOT NULL REFERENCES instruments(id) ON DELETE RESTRICT,
    PRIMARY KEY (instrument_id, part_id)
) WITHOUT ROWID;
CREATE INDEX part_instruments_part ON part_instruments(part_id);

-- File versions of a part. The highest version is the current one; older
-- versions stay readable for annotation sets drawn on them.
CREATE TABLE part_files (
    part_id     INTEGER NOT NULL REFERENCES parts(id) ON DELETE CASCADE,
    version     INTEGER NOT NULL,
    file_id     INTEGER NOT NULL REFERENCES files(id),
    uploaded_at INTEGER NOT NULL,
    PRIMARY KEY (part_id, version)
) WITHOUT ROWID;
CREATE INDEX part_files_file ON part_files(file_id);

-- Group "Trumpets" may read every part whose instrument is "Trumpet".
CREATE TABLE group_instrument_grants (
    group_id      INTEGER NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    instrument_id INTEGER NOT NULL REFERENCES instruments(id) ON DELETE CASCADE,
    PRIMARY KEY (group_id, instrument_id)
) WITHOUT ROWID;
CREATE INDEX group_instrument_grants_instrument ON group_instrument_grants(instrument_id);

-- A group may read every part of a score (conductor, section leaders).
CREATE TABLE group_score_grants (
    group_id INTEGER NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    score_id INTEGER NOT NULL REFERENCES scores(id) ON DELETE CASCADE,
    PRIMARY KEY (group_id, score_id)
) WITHOUT ROWID;
CREATE INDEX group_score_grants_score ON group_score_grants(score_id);

CREATE TABLE programmes (
    id         INTEGER PRIMARY KEY,
    title      TEXT NOT NULL,
    date       TEXT,
    notes      TEXT,
    created_at INTEGER NOT NULL
);

CREATE TABLE programme_items (
    programme_id INTEGER NOT NULL REFERENCES programmes(id) ON DELETE CASCADE,
    position     INTEGER NOT NULL,
    score_id     INTEGER NOT NULL REFERENCES scores(id) ON DELETE CASCADE,
    PRIMARY KEY (programme_id, position)
) WITHOUT ROWID;

-- Private annotation sets: one user, one part, one file version.
CREATE TABLE annotation_sets (
    id           INTEGER PRIMARY KEY,
    user_id      INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    part_id      INTEGER NOT NULL REFERENCES parts(id) ON DELETE CASCADE,
    file_id      INTEGER NOT NULL REFERENCES files(id),
    name         TEXT NOT NULL,
    programme_id INTEGER REFERENCES programmes(id) ON DELETE SET NULL,
    is_default   INTEGER NOT NULL DEFAULT 0,
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL,
    UNIQUE (user_id, part_id, name)
);

-- One layer per page: zstd(version byte + postcard(PageLayer)).
CREATE TABLE annotation_pages (
    set_id     INTEGER NOT NULL REFERENCES annotation_sets(id) ON DELETE CASCADE,
    page       INTEGER NOT NULL,
    revision   INTEGER NOT NULL,
    data       BLOB NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (set_id, page)
) WITHOUT ROWID;

-- ===== 0002_part_names.sql
-- Part names (a-part-names, R-LIB-010..012):
-- * `label` becomes an optional custom name (NULL = name derived from
--   instruments, key and voice); existing labels stay as custom names.
-- * `voice` becomes free text ("1a", "Solo"); numbers are kept as text.
-- * `transposition` is normalised to the key codes of
--   `musicus_api::naming::KEYS` where the spelling is unambiguous; other
--   values ("B", which is B-flat in German but B natural in English, or free
--   text) are kept as they are and stay editable. B natural is stored as "H".
-- * `part_fts` indexes the shown name of every part for the library search;
--   the server fills it (it needs the instrument names), see `part_index.rs`.

ALTER TABLE parts ADD COLUMN custom_label TEXT;
UPDATE parts SET custom_label = NULLIF(trim(label), '');
ALTER TABLE parts DROP COLUMN label;
ALTER TABLE parts RENAME COLUMN custom_label TO label;

ALTER TABLE parts ADD COLUMN voice_text TEXT;
UPDATE parts SET voice_text = CAST(voice AS TEXT) WHERE voice IS NOT NULL;
ALTER TABLE parts DROP COLUMN voice;
ALTER TABLE parts RENAME COLUMN voice_text TO voice;

UPDATE parts SET transposition = NULL WHERE trim(transposition) = '';
-- Mirrors musicus_api::naming::parse_key.
UPDATE parts SET transposition = m.code
FROM (
    SELECT id, CASE k
        WHEN 'bb' THEN 'Bb' WHEN 'b♭' THEN 'Bb' WHEN 'b-flat' THEN 'Bb' WHEN 'b flat' THEN 'Bb' WHEN 'bflat' THEN 'Bb'
        WHEN 'eb' THEN 'Eb' WHEN 'e♭' THEN 'Eb' WHEN 'e-flat' THEN 'Eb' WHEN 'e flat' THEN 'Eb' WHEN 'eflat' THEN 'Eb'
        WHEN 'es' THEN 'Eb'
        WHEN 'ab' THEN 'Ab' WHEN 'a♭' THEN 'Ab' WHEN 'a-flat' THEN 'Ab' WHEN 'a flat' THEN 'Ab' WHEN 'aflat' THEN 'Ab'
        WHEN 'as' THEN 'Ab'
        WHEN 'db' THEN 'Db' WHEN 'd♭' THEN 'Db' WHEN 'd-flat' THEN 'Db' WHEN 'd flat' THEN 'Db' WHEN 'dflat' THEN 'Db'
        WHEN 'des' THEN 'Db'
        WHEN 'gb' THEN 'Gb' WHEN 'g♭' THEN 'Gb' WHEN 'g-flat' THEN 'Gb' WHEN 'g flat' THEN 'Gb' WHEN 'gflat' THEN 'Gb'
        WHEN 'ges' THEN 'Gb'
        WHEN 'f' THEN 'F' WHEN 'c' THEN 'C' WHEN 'a' THEN 'A' WHEN 'd' THEN 'D' WHEN 'g' THEN 'G' WHEN 'e' THEN 'E'
        WHEN 'h' THEN 'H' WHEN 'b♮' THEN 'H' WHEN 'b natural' THEN 'H' WHEN 'bnatural' THEN 'H'
        ELSE NULL END AS code
    FROM (
        SELECT id, trim(CASE WHEN lower(trim(transposition)) LIKE 'in %' THEN lower(substr(trim(transposition), 4))
                             ELSE lower(trim(transposition)) END) AS k
        FROM parts WHERE transposition IS NOT NULL
    )
) AS m
WHERE parts.id = m.id AND m.code IS NOT NULL;

CREATE VIRTUAL TABLE part_fts USING fts5(
    name,
    tokenize = 'unicode61 remove_diacritics 2'
);
CREATE TRIGGER parts_fts_delete AFTER DELETE ON parts BEGIN
    DELETE FROM part_fts WHERE rowid = old.id;
END;

-- ===== 0003_system_layouts.sql
-- System view (F-READ-003, R-READ-010..R-READ-016):
-- * `file_layouts`: the music systems of every page of a file, detected once
--   per file by the background worker (`layout::worker`). `pages` is the JSON
--   array of `musicus_api::layout::PageLayout`; `algorithm` the detection
--   version that produced it (an older version is re-analysed). Rows exist
--   only for analysed files; a missing row means "pending".
-- * `users.reader_mode` / `users.staff_size`: the reader settings of a user,
--   kept on the server because the web client stores nothing on the device
--   (R-SEC-001).

CREATE TABLE file_layouts (
    file_id INTEGER PRIMARY KEY REFERENCES files (id) ON DELETE CASCADE,
    algorithm INTEGER NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('done', 'failed')),
    pages TEXT NOT NULL DEFAULT '[]',
    updated_at INTEGER NOT NULL
);

ALTER TABLE users ADD COLUMN reader_mode TEXT NOT NULL DEFAULT 'page' CHECK (reader_mode IN ('page', 'systems'));
ALTER TABLE users ADD COLUMN staff_size TEXT NOT NULL DEFAULT 'M' CHECK (staff_size IN ('S', 'M', 'L', 'XL'));

-- ===== 0004_layout_corrections.sql
-- Librarians' corrections of the system layout (R-READ-016): one row per
-- corrected page of a file, JSON `musicus_api::layout::PageCorrection`.
-- Kept apart from `file_layouts`, so a new detection run never overwrites
-- them; the server applies them when it serves a layout. Shared by all
-- members (the file is the same for everyone).

CREATE TABLE layout_corrections (
    file_id INTEGER NOT NULL REFERENCES files (id) ON DELETE CASCADE,
    page INTEGER NOT NULL,
    data TEXT NOT NULL,
    updated_by INTEGER REFERENCES users (id) ON DELETE SET NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (file_id, page)
) WITHOUT ROWID;

-- ===== 0005_part_voices.sql
-- Several voices per part (a-part-voices, R-LIB-002, R-LIB-010):
-- * `part_voices` holds the ordered voices of a part ("Clarinet 1 & 2" =
--   voices "1", "2" at positions 0, 1). Relational, not a JSON column, so a
--   later grant per voice (R-ACL-005, proposed) can join on it; the index on
--   `voice` serves that lookup. Voices compare ignoring case, like the
--   server's de-duplication.
-- * The old single `parts.voice` becomes the only entry of the list; it is
--   not split ("1+2" stays one voice). Nothing was released, so no further
--   conversion is needed.
-- * `part_fts` is emptied so the server rebuilds it on start from the new
--   table (`part_index::rebuild_if_needed`).

CREATE TABLE part_voices (
    part_id  INTEGER NOT NULL REFERENCES parts(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    voice    TEXT NOT NULL COLLATE NOCASE,
    PRIMARY KEY (part_id, position),
    UNIQUE (part_id, voice)
) WITHOUT ROWID;
CREATE INDEX part_voices_voice ON part_voices(voice);

INSERT INTO part_voices (part_id, position, voice)
SELECT id, 0, trim(voice) FROM parts WHERE voice IS NOT NULL AND trim(voice) <> '';

ALTER TABLE parts DROP COLUMN voice;

DELETE FROM part_fts;

-- ===== 0006_archive_number.sql
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

-- ===== 0007_import.sql
-- Mass import of a sheet archive (F-LIB-004, a-import-staging).
-- A batch = one selected folder. Its files are uploaded into the blob store
-- (`files` rows, but no `part_files`, so nobody can read them through the
-- library) and proposed as pieces and parts (`import_items`). Pieces and
-- parts are created only when the librarian commits them.

CREATE TABLE import_batches (
    id         INTEGER PRIMARY KEY,
    -- Name of the selected folder.
    name       TEXT NOT NULL,
    pattern    TEXT NOT NULL,
    -- Naming language: German key letters (B = B-flat) only for 'de'.
    language   TEXT NOT NULL CHECK (language IN ('de', 'en')),
    created_by INTEGER REFERENCES users(id) ON DELETE SET NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

-- Every PDF of the folder, with the hash the browser computed.
CREATE TABLE import_files (
    id         INTEGER PRIMARY KEY,
    batch_id   INTEGER NOT NULL REFERENCES import_batches(id) ON DELETE CASCADE,
    -- Below the selected folder, '/'-separated.
    path       TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    sha256     TEXT NOT NULL,
    -- pending: content not received; uploaded: in the blob store (file_id);
    -- duplicate: content already in the library or the batch, skipped;
    -- failed: content rejected (message).
    state      TEXT NOT NULL CHECK (state IN ('pending', 'uploaded', 'duplicate', 'failed')),
    file_id    INTEGER REFERENCES files(id),
    message    TEXT,
    UNIQUE (batch_id, path)
);
CREATE INDEX import_files_sha ON import_files(sha256);
CREATE INDEX import_files_file ON import_files(file_id);

-- Proposed pieces (kind 'piece') and their parts (kind 'part', one per file).
CREATE TABLE import_items (
    id            INTEGER PRIMARY KEY,
    batch_id      INTEGER NOT NULL REFERENCES import_batches(id) ON DELETE CASCADE,
    kind          TEXT NOT NULL CHECK (kind IN ('piece', 'part')),
    piece_id      INTEGER REFERENCES import_items(id) ON DELETE CASCADE,
    file_id       INTEGER REFERENCES import_files(id) ON DELETE CASCADE,
    -- Grouping key of a piece / path of a part: keeps the librarian's
    -- changes when the proposals are computed again.
    key           TEXT NOT NULL,
    position      INTEGER NOT NULL,
    -- JSON musicus_api::import::review::PieceGuess / PartGuess (values with source).
    proposal      TEXT NOT NULL,
    -- JSON PieceOverrides / PartOverrides (the librarian's changes).
    overrides     TEXT NOT NULL DEFAULT '{}',
    state         TEXT NOT NULL DEFAULT 'pending' CHECK (state IN ('pending', 'committed')),
    -- After the commit: the piece the parts went to, the created part.
    score_id      INTEGER REFERENCES scores(id) ON DELETE SET NULL,
    part_id       INTEGER REFERENCES parts(id) ON DELETE SET NULL,
    -- 1 if the commit created score_id (undo deletes it), 0 if it added parts to an existing piece.
    created_score INTEGER NOT NULL DEFAULT 0,
    committed_at  INTEGER
);
CREATE INDEX import_items_batch ON import_items(batch_id, kind);
CREATE INDEX import_items_piece ON import_items(piece_id);
CREATE INDEX import_items_file ON import_items(file_id);

-- A word of file names (folded: lower case, no diacritics) that means an
-- instrument, chosen once by a librarian and applied to every import.
CREATE TABLE instrument_aliases (
    alias         TEXT PRIMARY KEY,
    instrument_id INTEGER NOT NULL REFERENCES instruments(id) ON DELETE CASCADE,
    created_by    INTEGER REFERENCES users(id) ON DELETE SET NULL,
    created_at    INTEGER NOT NULL
) WITHOUT ROWID;

-- ===== 0008_import_file_problem.sql
-- Staged import files: the reason a file was skipped or rejected is stored as
-- the JSON of an error code (`musicus_api::ErrorCode`), not as English text;
-- the client renders it in the user's language (a-error-codes, R-I18N-004).
ALTER TABLE import_files RENAME COLUMN message TO problem;

UPDATE import_files
   SET problem = json_object('code', 'duplicate_in_library', 'args', json_object('title', substr(problem, 25)))
 WHERE problem LIKE 'already in the library: %';
UPDATE import_files
   SET problem = json_object('code', 'duplicate_in_batch', 'args', json_object('path', substr(problem, 14)))
 WHERE problem LIKE 'same file as %';
UPDATE import_files SET problem = json_object('code', 'not_pdf') WHERE problem = 'only PDF files are supported';
UPDATE import_files SET problem = json_object('code', 'unknown') WHERE problem IS NOT NULL AND json_valid(problem) = 0;

-- ===== 0009_i18n_names.sql
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

-- ===== 0010_german_key_spellings.sql
-- German ships (locales/de): the key spellings of the search index grow by the
-- German names (B, Es, As, Des, Ges; R-I18N-007). `part_fts` is emptied so
-- `part_index::rebuild_if_needed` rebuilds it on start with every spelling.
-- Every change of `musicus_i18n::KEY_SPELLINGS` needs such a migration
-- (guarded by `part_index::tests::indexed_key_spellings_are_current`).
DELETE FROM part_fts;

-- ===== sqlx bookkeeping (as written by sqlx 0.9)
CREATE TABLE _sqlx_migrations (
    version BIGINT PRIMARY KEY,
    description TEXT NOT NULL,
    installed_on TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    success BOOLEAN NOT NULL,
    checksum BLOB NOT NULL,
    execution_time BIGINT NOT NULL
);
INSERT INTO _sqlx_migrations (version, description, installed_on, success, checksum, execution_time) VALUES (1, 'init', '2026-11-21 22:05:16', 1, X'07EC9C0A12A1C115F6A8325FF1ACD3AD6EBE1F343CA91136FCD1CDDC5EDA24F19AFAAE7F57BD5E745B15D062E5DA2288', 3878142);
INSERT INTO _sqlx_migrations (version, description, installed_on, success, checksum, execution_time) VALUES (2, 'part names', '2026-11-21 22:05:16', 1, X'65D2CC74E6F9ABF5A75088663E5B592E865A1A950BE0D8E67F6FDD8F3B31F30E4CF9FA75629ED090B6E0E20409B20CDE', 13524171);
INSERT INTO _sqlx_migrations (version, description, installed_on, success, checksum, execution_time) VALUES (3, 'system layouts', '2026-11-21 22:05:16', 1, X'D5366B92A7E412013BDD47B999B604D9FDFA40138EC7686FA96BCB37B4FFDCCBAAEBEFB1525D2AD0CF0B99016C50A139', 2059806);
INSERT INTO _sqlx_migrations (version, description, installed_on, success, checksum, execution_time) VALUES (4, 'layout corrections', '2026-11-21 22:05:16', 1, X'485BDC7B432EE8B765FFB2F02A2FAFE341C067BCB5727EF5CBF3B2B194FF9D0A920A91943D2D3A15D373651655D1E2AE', 377337);
INSERT INTO _sqlx_migrations (version, description, installed_on, success, checksum, execution_time) VALUES (5, 'part voices', '2026-11-21 22:05:16', 1, X'877B175BC4D48891C87B51AC11BF684BFB74C304F3A63B7484540D4D5BC16D54D51F019C861ED80151C0F02A102E4D06', 3888953);
INSERT INTO _sqlx_migrations (version, description, installed_on, success, checksum, execution_time) VALUES (6, 'archive number', '2026-11-21 22:05:16', 1, X'1BE2B6DA062B54F6AC70B35B361802761E438F846013E6C8EFEA6FDD948AFFA50423DC6ED36B162CEF6946C69EA27D93', 3487039);
INSERT INTO _sqlx_migrations (version, description, installed_on, success, checksum, execution_time) VALUES (7, 'import', '2026-11-21 22:05:16', 1, X'A6D843AD5E36AFF4B7ADFF7CF8F892513AA4C62DBFEA3FE4CFC9DDC28F5D4DB8D46DEBBEA6A96BB00C5CCE75677C1496', 1292567);
INSERT INTO _sqlx_migrations (version, description, installed_on, success, checksum, execution_time) VALUES (8, 'import file problem', '2026-11-21 22:05:16', 1, X'0691B3D168EA3C9C39948CD8E10BD0DFABC0403887E8441E8134083D557F70FAD219DBE03097EFB2D06999F54781B26C', 4799554);
INSERT INTO _sqlx_migrations (version, description, installed_on, success, checksum, execution_time) VALUES (9, 'i18n names', '2026-11-21 22:05:16', 1, X'8E3A6F4F5F68EBEDC2B8C2A48B28953D5130237C8C1012E68B08EE515EB2E78291B4CB82A2BE08C39C108DEA6434EA9B', 3499422);
INSERT INTO _sqlx_migrations (version, description, installed_on, success, checksum, execution_time) VALUES (10, 'german key spellings', '2026-11-21 22:05:16', 1, X'A4C5005BA7A0C78B0A303454DCD0E23F044912920A42C8277DADF53441049AD7A36C291A6EE41E85369C74D5A9A41BA3', 210720);
