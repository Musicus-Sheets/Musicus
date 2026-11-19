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
