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
