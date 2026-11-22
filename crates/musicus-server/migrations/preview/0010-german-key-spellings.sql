-- Development migration 0010 of the sqlx::migrate! time, as a preview (a-update-runner).
-- Text unchanged. Destructive steps in the additive part are acceptable before 1.0:
-- there is no downgrade guarantee for development databases.
-- @additive
-- German ships (locales/de): the key spellings of the search index grow by the
-- German names (B, Es, As, Des, Ges; R-I18N-007). `part_fts` is emptied so
-- `part_index::rebuild_if_needed` rebuilds it on start with every spelling.
-- Every change of `musicus_i18n::KEY_SPELLINGS` needs such a migration
-- (guarded by `part_index::tests::indexed_key_spellings_are_current`).
DELETE FROM part_fts;
