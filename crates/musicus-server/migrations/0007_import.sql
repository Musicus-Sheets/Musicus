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
