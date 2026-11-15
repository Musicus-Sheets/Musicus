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
