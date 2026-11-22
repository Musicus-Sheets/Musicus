-- Development migration 0004 of the sqlx::migrate! time, as a preview (a-update-runner).
-- Text unchanged. Destructive steps in the additive part are acceptable before 1.0:
-- there is no downgrade guarantee for development databases.
-- @additive
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
