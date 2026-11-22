-- Musicus 1.0.0: the baseline schema. Every guarantee for updates and
-- downgrades starts here (d-update-model). It replaced the development
-- migrations 0001–0010 of the `sqlx::migrate!` time and gives exactly their
-- schema (test `baseline_equals_the_development_migrations`); databases that
-- applied them are recorded once as having this update (a-update-runner).
-- Times are Unix seconds. Sheet files live in the blob store on disk; this
-- database only references them by SHA-256.
-- Unreleased until `1.0` is listed in releases.txt: until then previews may
-- still be merged into it (scripts/release-migrations.py).
-- @additive

CREATE TABLE users (
    id            INTEGER PRIMARY KEY,
    username      TEXT NOT NULL UNIQUE COLLATE NOCASE,
    display_name  TEXT NOT NULL,
    -- NULL until the user has accepted their invite.
    password_hash TEXT,
    role          TEXT NOT NULL CHECK (role IN ('member', 'librarian', 'admin')),
    disabled      INTEGER NOT NULL DEFAULT 0,
    created_at    INTEGER NOT NULL,
    -- Reader settings, kept on the server because the web client stores
    -- nothing on the device (R-SEC-001).
    reader_mode   TEXT NOT NULL DEFAULT 'page' CHECK (reader_mode IN ('page', 'systems')),
    staff_size    TEXT NOT NULL DEFAULT 'M' CHECK (staff_size IN ('S', 'M', 'L', 'XL'))
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

-- A score is the work or arrangement (a "piece" in the user interface).
CREATE TABLE scores (
    id             INTEGER PRIMARY KEY,
    title          TEXT NOT NULL,
    subtitle       TEXT,
    composer       TEXT,
    arranger       TEXT,
    genre          TEXT,
    -- JSON array of strings.
    tags           TEXT NOT NULL DEFAULT '[]',
    notes          TEXT,
    created_at     INTEGER NOT NULL,
    updated_at     INTEGER NOT NULL,
    -- Archive number (R-LIB-015, a-archive-number): free text such as "0304"
    -- or "A-12", not unique; the import uses it only to suggest a match.
    archive_number TEXT,
    -- What the search indexes for it ("0304 304", "A-12 A12"), written by the
    -- server (musicus_api::naming::archive_search_text) whenever the number changes.
    archive_search TEXT
);

-- Full-text search over the score metadata. remove_diacritics makes
-- "Fucik" find "Fučík".
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

-- A part is one sheet of a score, e.g. "2nd Trumpet in Bb" (a-part-names):
-- `transposition` is a key code of musicus_api::naming::KEYS (Bb, Eb, …, H)
-- or a free-text value of the development time; `label` an optional custom
-- name (NULL = derived from instruments, key and voices). The column order
-- is that of the development migrations.
CREATE TABLE parts (
    id            INTEGER PRIMARY KEY,
    score_id      INTEGER NOT NULL REFERENCES scores(id) ON DELETE CASCADE,
    transposition TEXT,
    notes         TEXT,
    created_at    INTEGER NOT NULL,
    label         TEXT
);
CREATE INDEX parts_score ON parts(score_id);

-- The ordered voices of a part ("Clarinet 1 & 2" = voices "1", "2" at
-- positions 0, 1; a-part-voices). Relational, so a later grant per voice
-- (R-ACL-005) can join on it.
CREATE TABLE part_voices (
    part_id  INTEGER NOT NULL REFERENCES parts(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    voice    TEXT NOT NULL COLLATE NOCASE,
    PRIMARY KEY (part_id, position),
    UNIQUE (part_id, voice)
) WITHOUT ROWID;
CREATE INDEX part_voices_voice ON part_voices(voice);

-- Search index of the parts, filled by the server (it needs the instrument
-- names; part_index.rs). Rows go with their part by trigger.
CREATE VIRTUAL TABLE part_fts USING fts5(
    name,
    tokenize = 'unicode61 remove_diacritics 2'
);
CREATE TRIGGER parts_fts_delete AFTER DELETE ON parts BEGIN
    DELETE FROM part_fts WHERE rowid = old.id;
END;

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

-- Private annotation sets: one user, one part, one file version. `name` NULL
-- = the set the reader created without asking, shown as "My annotations" in
-- the UI language; the partial index keeps one such set per user and part.
CREATE TABLE annotation_sets (
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
CREATE UNIQUE INDEX annotation_sets_unnamed ON annotation_sets (user_id, part_id) WHERE name IS NULL;

-- One layer per page: zstd(version byte + postcard(PageLayer)).
CREATE TABLE annotation_pages (
    set_id     INTEGER NOT NULL REFERENCES annotation_sets(id) ON DELETE CASCADE,
    page       INTEGER NOT NULL,
    revision   INTEGER NOT NULL,
    data       BLOB NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (set_id, page)
) WITHOUT ROWID;

-- System view (F-READ-003): the music systems of every page of a file,
-- detected once per file by the background worker (layout::worker). `pages`
-- is the JSON array of musicus_api::layout::PageLayout; `algorithm` the
-- detection version that produced it (an older version is re-analysed). A
-- missing row means "pending".
CREATE TABLE file_layouts (
    file_id INTEGER PRIMARY KEY REFERENCES files (id) ON DELETE CASCADE,
    algorithm INTEGER NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('done', 'failed')),
    pages TEXT NOT NULL DEFAULT '[]',
    updated_at INTEGER NOT NULL
);

-- Librarians' corrections of the system layout (R-READ-016): one row per
-- corrected page, JSON musicus_api::layout::PageCorrection. Kept apart from
-- file_layouts, so a new detection run never overwrites them.
CREATE TABLE layout_corrections (
    file_id INTEGER NOT NULL REFERENCES files (id) ON DELETE CASCADE,
    page INTEGER NOT NULL,
    data TEXT NOT NULL,
    updated_by INTEGER REFERENCES users (id) ON DELETE SET NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (file_id, page)
) WITHOUT ROWID;

-- Mass import of a sheet archive (F-LIB-004, a-import-staging). A batch = one
-- selected folder. Its files are uploaded into the blob store (`files` rows,
-- but no `part_files`, so nobody can read them through the library) and
-- proposed as pieces and parts (`import_items`). Pieces and parts are created
-- only when the librarian commits them.
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
    -- failed: content rejected (problem).
    state      TEXT NOT NULL CHECK (state IN ('pending', 'uploaded', 'duplicate', 'failed')),
    file_id    INTEGER REFERENCES files(id),
    -- Why the file was skipped or rejected: JSON of a musicus_api::ErrorCode
    -- (a-error-codes), rendered by the client in the user's language.
    problem    TEXT,
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
