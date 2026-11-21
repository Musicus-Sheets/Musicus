-- German ships (locales/de): the key spellings of the search index grow by the
-- German names (B, Es, As, Des, Ges; R-I18N-007). `part_fts` is emptied so
-- `part_index::rebuild_if_needed` rebuilds it on start with every spelling.
-- Every change of `musicus_i18n::KEY_SPELLINGS` needs such a migration
-- (guarded by `part_index::tests::indexed_key_spellings_are_current`).
DELETE FROM part_fts;
