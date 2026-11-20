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
