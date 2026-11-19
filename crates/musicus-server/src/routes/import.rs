//! Mass import of a sheet archive (F-LIB-004): batches, staged files,
//! proposals, review, commit, undo and instrument aliases. Librarians only.
//!
//! Flow (a-import-staging): the browser registers the file list with the
//! hashes it computed (`register_files`); the server marks duplicates,
//! computes the proposals from the paths (`repropose`, pure rules in
//! `musicus_api::import`) and names the files it still needs; the browser
//! uploads them (`upload_content`). Staged files are blobs with a `files`
//! row but no `part_files`, so `/api/files/{sha}` never serves them
//! (`files::readable_file` needs a part); the review shows them through the
//! batch's own routes. The review is resolved on every read against the
//! current instruments, aliases, pieces and groups. `commit` creates each
//! piece in its own short write transaction; `undo` removes what a batch created.

use crate::{
    access::librarian_flag,
    auth::AuthUser,
    blobs::{UploadError, is_sha256_hex},
    db::{json_ids, now},
    error::{ApiResult, AppError, required},
    part_index, part_visible,
    routes::{files::serve_blob, library},
    state::{App, AppState},
};
use axum::{
    Json,
    body::Body,
    extract::{Path, Query, Request, State},
    http::StatusCode,
    response::Response,
};
use musicus_api::{
    CommitImport, CommittedImport, Group, Id, ImportBatch, ImportFileCounts, ImportItemError, ImportReview,
    ImportUpload, Instrument, InstrumentAlias, InstrumentAliasInput, NewImportBatch, PartInput, RegisterImportFiles,
    RegisteredImportFiles, ScoreInput, UndoneImport, UpdateImportBatch, UpdateImportItems,
    import::{
        Language, is_pdf_name,
        parts::alias_key,
        pattern::Pattern,
        review::{
            Alias, FileState, ItemState, Library, PartGuess, PartInput as ReviewPartInput, PartOverrides, PieceGuess,
            PieceOverrides, ReviewPiece, ScoreRef, StagedFile, Target, propose, review_piece,
        },
    },
    naming,
};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};

/// Files per registration request.
const MAX_FILES: usize = 20_000;
/// Items per bulk edit.
const MAX_ITEMS: usize = 5_000;

#[derive(sqlx::FromRow)]
struct BatchRow {
    id: Id,
    name: String,
    pattern: String,
    language: String,
    created_at: i64,
    created_by: Option<String>,
}

impl BatchRow {
    fn language(&self) -> Language {
        Language::parse(&self.language).unwrap_or_default()
    }

    fn pattern(&self) -> ApiResult<Pattern> {
        Pattern::parse(&self.pattern).map_err(|e| AppError::BadRequest(e.to_string()))
    }
}

async fn batch_row(app: &App, id: Id) -> ApiResult<BatchRow> {
    sqlx::query_as(
        "SELECT b.id, b.name, b.pattern, b.language, b.created_at, u.display_name AS created_by
         FROM import_batches b LEFT JOIN users u ON u.id = b.created_by WHERE b.id = ?1",
    )
    .bind(id)
    .fetch_optional(&app.db.read)
    .await?
    .ok_or(AppError::NotFound)
}

async fn batch_info(app: &App, row: &BatchRow) -> ApiResult<ImportBatch> {
    let (total, pending, uploaded, duplicate, failed, unmatched): (i64, i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), COALESCE(SUM(state = 'pending'), 0), COALESCE(SUM(state = 'uploaded'), 0),
                COALESCE(SUM(state = 'duplicate'), 0), COALESCE(SUM(state = 'failed'), 0),
                COALESCE(SUM(state <> 'duplicate' AND NOT EXISTS (SELECT 1 FROM import_items i WHERE i.file_id = f.id)), 0)
         FROM import_files f WHERE f.batch_id = ?1",
    )
    .bind(row.id)
    .fetch_one(&app.db.read)
    .await?;
    let (pieces, committed): (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), COALESCE(SUM(state = 'committed'), 0) FROM import_items WHERE batch_id = ?1 AND kind = 'piece'",
    )
    .bind(row.id)
    .fetch_one(&app.db.read)
    .await?;
    Ok(ImportBatch {
        id: row.id,
        name: row.name.clone(),
        pattern: row.pattern.clone(),
        language: row.language(),
        created_at: row.created_at,
        created_by: row.created_by.clone(),
        files: ImportFileCounts { total, pending, uploaded, duplicate, failed, unmatched },
        pieces,
        committed,
    })
}

pub async fn list_batches(State(app): State<AppState>, user: AuthUser) -> ApiResult<Json<Vec<ImportBatch>>> {
    user.require_librarian()?;
    let rows: Vec<BatchRow> = sqlx::query_as(
        "SELECT b.id, b.name, b.pattern, b.language, b.created_at, u.display_name AS created_by
         FROM import_batches b LEFT JOIN users u ON u.id = b.created_by ORDER BY b.id DESC",
    )
    .fetch_all(&app.db.read)
    .await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        out.push(batch_info(&app, row).await?);
    }
    Ok(Json(out))
}

fn check_pattern(pattern: &str) -> ApiResult<String> {
    let pattern = required("pattern", pattern, 300)?;
    Pattern::parse(&pattern).map_err(|e| AppError::BadRequest(e.to_string()))?;
    Ok(pattern)
}

pub async fn create_batch(
    State(app): State<AppState>,
    user: AuthUser,
    Json(input): Json<NewImportBatch>,
) -> ApiResult<Json<ImportBatch>> {
    user.require_librarian()?;
    let name = required("name", &input.name, 200)?;
    let pattern = check_pattern(&input.pattern)?;
    let id: Id = sqlx::query_scalar(
        "INSERT INTO import_batches (name, pattern, language, created_by, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?5) RETURNING id",
    )
    .bind(name)
    .bind(pattern)
    .bind(input.language.as_str())
    .bind(user.id)
    .bind(now())
    .fetch_one(&app.db.write)
    .await?;
    let row = batch_row(&app, id).await?;
    Ok(Json(batch_info(&app, &row).await?))
}

/// Renames the batch or reads its file names again with another pattern or language.
pub async fn update_batch(
    State(app): State<AppState>,
    user: AuthUser,
    Path(id): Path<Id>,
    Json(input): Json<UpdateImportBatch>,
) -> ApiResult<Json<ImportBatch>> {
    user.require_librarian()?;
    let row = batch_row(&app, id).await?;
    let name = match &input.name {
        Some(n) => required("name", n, 200)?,
        None => row.name.clone(),
    };
    let pattern_text = match &input.pattern {
        Some(p) => check_pattern(p)?,
        None => row.pattern.clone(),
    };
    let language = input.language.unwrap_or(row.language());
    let pattern = Pattern::parse(&pattern_text).map_err(|e| AppError::BadRequest(e.to_string()))?;
    let mut tx = app.db.write.begin().await?;
    sqlx::query("UPDATE import_batches SET name = ?2, pattern = ?3, language = ?4, updated_at = ?5 WHERE id = ?1")
        .bind(id)
        .bind(name)
        .bind(&pattern_text)
        .bind(language.as_str())
        .bind(now())
        .execute(&mut *tx)
        .await?;
    if pattern_text != row.pattern || language != row.language() {
        repropose(&mut tx, id, &pattern, language).await?;
    }
    tx.commit().await?;
    let row = batch_row(&app, id).await?;
    Ok(Json(batch_info(&app, &row).await?))
}

/// Forgets the batch (staging data). Committed pieces stay in the library;
/// uploaded files stay in the blob store (R-FILE-004).
pub async fn delete_batch(State(app): State<AppState>, user: AuthUser, Path(id): Path<Id>) -> ApiResult<StatusCode> {
    user.require_librarian()?;
    let done = sqlx::query("DELETE FROM import_batches WHERE id = ?1").bind(id).execute(&app.db.write).await?;
    if done.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

fn check_path(path: &str) -> ApiResult<String> {
    let path = path.trim().trim_matches('/').replace('\\', "/");
    if path.is_empty()
        || path.chars().count() > 1000
        || path.split('/').any(|s| s.is_empty() || s == "." || s == "..")
        || path.chars().any(char::is_control)
    {
        return Err(AppError::BadRequest(format!("invalid path \"{path}\"")));
    }
    if !is_pdf_name(&path) {
        return Err(AppError::BadRequest(format!("not a PDF file name: \"{path}\"")));
    }
    Ok(path)
}

/// Registers the file list of the selected folder (again, after an
/// interruption): new files are added, known files keep their state.
/// Content already in the library or elsewhere in the batch is a duplicate
/// and never imported twice; content already on the server (e.g. from
/// another batch) needs no upload. Answers with the files to upload.
pub async fn register_files(
    State(app): State<AppState>,
    user: AuthUser,
    Path(batch_id): Path<Id>,
    Json(input): Json<RegisterImportFiles>,
) -> ApiResult<Json<RegisteredImportFiles>> {
    user.require_librarian()?;
    let row = batch_row(&app, batch_id).await?;
    let pattern = row.pattern()?;
    if input.files.len() > MAX_FILES {
        return Err(AppError::BadRequest(format!("at most {MAX_FILES} files per request")));
    }
    let mut files = Vec::with_capacity(input.files.len());
    let mut seen = HashSet::new();
    for f in &input.files {
        let path = check_path(&f.path)?;
        let sha = f.sha256.trim().to_ascii_lowercase();
        if !is_sha256_hex(&sha) || f.size_bytes < 0 {
            return Err(AppError::BadRequest(format!("invalid hash or size for \"{path}\"")));
        }
        if seen.insert(path.clone()) {
            files.push((path, f.size_bytes, sha));
        }
    }

    let mut tx = app.db.write.begin().await?;
    for (path, size, sha) in &files {
        let existing: Option<(Id, String, String)> =
            sqlx::query_as("SELECT id, sha256, state FROM import_files WHERE batch_id = ?1 AND path = ?2")
                .bind(batch_id)
                .bind(path)
                .fetch_optional(&mut *tx)
                .await?;
        let committed = match existing {
            Some((id, _, _)) => {
                sqlx::query_scalar::<_, i64>("SELECT 1 FROM import_items WHERE file_id = ?1 AND state = 'committed'")
                    .bind(id)
                    .fetch_optional(&mut *tx)
                    .await?
                    .is_some()
            }
            None => false,
        };
        match existing {
            // Unchanged, or already imported: nothing to do.
            Some((_, known, _)) if known == *sha || committed => continue,
            _ => {}
        }
        // Same content already in the library, or in this batch under another path?
        let in_library: Option<String> = sqlx::query_scalar(
            "SELECT s.title FROM files f JOIN part_files pf ON pf.file_id = f.id JOIN parts p ON p.id = pf.part_id
             JOIN scores s ON s.id = p.score_id WHERE f.sha256 = ?1 LIMIT 1",
        )
        .bind(sha)
        .fetch_optional(&mut *tx)
        .await?;
        let in_batch: Option<String> = sqlx::query_scalar(
            "SELECT path FROM import_files WHERE batch_id = ?1 AND sha256 = ?2 AND path <> ?3 AND state <> 'duplicate' LIMIT 1",
        )
        .bind(batch_id)
        .bind(sha)
        .bind(path)
        .fetch_optional(&mut *tx)
        .await?;
        let stored: Option<Id> =
            sqlx::query_scalar("SELECT id FROM files WHERE sha256 = ?1").bind(sha).fetch_optional(&mut *tx).await?;
        let (state, file_id, message) = match (in_library, in_batch, stored) {
            (Some(title), _, _) => (FileState::Duplicate, None, Some(format!("already in the library: {title}"))),
            (None, Some(other), _) => (FileState::Duplicate, None, Some(format!("same file as {other}"))),
            (None, None, Some(id)) => (FileState::Uploaded, Some(id), None),
            (None, None, None) => (FileState::Pending, None, None),
        };
        sqlx::query(
            "INSERT INTO import_files (batch_id, path, size_bytes, sha256, state, file_id, message)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT (batch_id, path) DO UPDATE SET size_bytes = ?3, sha256 = ?4, state = ?5, file_id = ?6, message = ?7",
        )
        .bind(batch_id)
        .bind(path)
        .bind(size)
        .bind(sha)
        .bind(state.as_str())
        .bind(file_id)
        .bind(message)
        .execute(&mut *tx)
        .await?;
    }
    repropose(&mut tx, batch_id, &pattern, row.language()).await?;
    sqlx::query("UPDATE import_batches SET updated_at = ?2 WHERE id = ?1")
        .bind(batch_id)
        .bind(now())
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    let upload: Vec<(Id, String)> = sqlx::query_as(
        "SELECT id, path FROM import_files WHERE batch_id = ?1 AND state IN ('pending', 'failed') ORDER BY path",
    )
    .bind(batch_id)
    .fetch_all(&app.db.read)
    .await?;
    let info = batch_info(&app, &row).await?;
    Ok(Json(RegisteredImportFiles {
        upload: upload.into_iter().map(|(id, path)| ImportUpload { id, path }).collect(),
        files: info.files,
    }))
}

#[derive(sqlx::FromRow)]
struct OldItem {
    kind: String,
    key: String,
    overrides: String,
    state: String,
    piece_state: Option<String>,
}

/// Computes the proposals of a batch again from its file paths (in the
/// caller's transaction). Committed items stay; pending ones are replaced,
/// keeping the librarian's changes by grouping key (pieces) and path (parts).
async fn repropose(
    conn: &mut sqlx::SqliteConnection,
    batch_id: Id,
    pattern: &Pattern,
    language: Language,
) -> ApiResult<()> {
    let old: Vec<OldItem> = sqlx::query_as(
        "SELECT i.kind, i.key, i.overrides, i.state, pc.state AS piece_state
         FROM import_items i LEFT JOIN import_items pc ON pc.id = i.piece_id WHERE i.batch_id = ?1",
    )
    .bind(batch_id)
    .fetch_all(&mut *conn)
    .await?;
    // Files already imported (or left out of a committed piece) are not proposed again.
    let done: HashSet<&str> = old
        .iter()
        .filter(|i| i.kind == "part" && (i.state == "committed" || i.piece_state.as_deref() == Some("committed")))
        .map(|i| i.key.as_str())
        .collect();
    let piece_overrides: HashMap<&str, &str> = old
        .iter()
        .filter(|i| i.kind == "piece" && i.state == "pending")
        .map(|i| (i.key.as_str(), i.overrides.as_str()))
        .collect();
    let part_overrides: HashMap<&str, &str> = old
        .iter()
        .filter(|i| i.kind == "part" && i.state == "pending")
        .map(|i| (i.key.as_str(), i.overrides.as_str()))
        .collect();
    let files: Vec<(Id, String)> =
        sqlx::query_as("SELECT id, path FROM import_files WHERE batch_id = ?1 AND state <> 'duplicate' ORDER BY id")
            .bind(batch_id)
            .fetch_all(&mut *conn)
            .await?;
    let ids: HashMap<&str, Id> = files.iter().map(|(id, p)| (p.as_str(), *id)).collect();
    let paths: Vec<String> = files.iter().map(|(_, p)| p.clone()).filter(|p| !done.contains(p.as_str())).collect();
    let (pieces, _unmatched) = propose(pattern, language, &paths);

    // Pending pieces go (their parts with them, ON DELETE CASCADE).
    sqlx::query("DELETE FROM import_items WHERE batch_id = ?1 AND kind = 'piece' AND state = 'pending'")
        .bind(batch_id)
        .execute(&mut *conn)
        .await?;
    let first: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(position) + 1, 0) FROM import_items WHERE batch_id = ?1")
        .bind(batch_id)
        .fetch_one(&mut *conn)
        .await?;
    for (n, p) in pieces.iter().enumerate() {
        let over = piece_overrides.get(p.piece.key.as_str()).copied().unwrap_or("{}");
        let piece_id: Id = sqlx::query_scalar(
            "INSERT INTO import_items (batch_id, kind, key, position, proposal, overrides)
             VALUES (?1, 'piece', ?2, ?3, ?4, ?5) RETURNING id",
        )
        .bind(batch_id)
        .bind(&p.piece.key)
        .bind(first + n as i64)
        .bind(to_json(&p.piece))
        .bind(over)
        .fetch_one(&mut *conn)
        .await?;
        for (k, part) in p.parts.iter().enumerate() {
            let over = part_overrides.get(part.path.as_str()).copied().unwrap_or("{}");
            sqlx::query(
                "INSERT INTO import_items (batch_id, kind, piece_id, file_id, key, position, proposal, overrides)
                 VALUES (?1, 'part', ?2, ?3, ?4, ?5, ?6, ?7)",
            )
            .bind(batch_id)
            .bind(piece_id)
            .bind(ids.get(part.path.as_str()))
            .bind(&part.path)
            .bind(k as i64)
            .bind(to_json(part))
            .bind(over)
            .execute(&mut *conn)
            .await?;
        }
    }
    Ok(())
}

fn to_json<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).expect("proposals serialise")
}

/// Typical staves per system of a file (same rule as
/// `musicus_api::import::review::staves_per_system`, in SQL so the review
/// does not load every layout).
macro_rules! staves {
    () => {
        "(SELECT CAST(json_extract(r.value, '$.staves') AS INTEGER) AS n
          FROM file_layouts l, json_each(l.pages) pg, json_each(pg.value, '$.regions') r
          WHERE l.file_id = fl.id AND l.state = 'done' AND json_extract(pg.value, '$.full_page') = 0
            AND json_extract(r.value, '$.kind') = 'system' AND json_extract(r.value, '$.staves') > 0
          GROUP BY n ORDER BY COUNT(*) DESC, n DESC LIMIT 1)"
    };
}

#[derive(sqlx::FromRow)]
struct FileRow {
    id: Id,
    path: String,
    size_bytes: i64,
    sha256: String,
    state: String,
    page_count: Option<i64>,
    has_thumbnail: Option<bool>,
    staves: Option<i64>,
    message: Option<String>,
    matched: bool,
}

impl From<FileRow> for StagedFile {
    fn from(r: FileRow) -> Self {
        StagedFile {
            id: r.id,
            path: r.path,
            size_bytes: r.size_bytes,
            sha256: r.sha256,
            state: FileState::parse(&r.state),
            page_count: r.page_count,
            has_thumbnail: r.has_thumbnail.unwrap_or(false),
            staves: r.staves.and_then(|n| u32::try_from(n).ok()),
            message: r.message,
        }
    }
}

async fn staged_files(app: &App, batch_id: Id, file_id: Option<Id>) -> ApiResult<Vec<FileRow>> {
    Ok(sqlx::query_as(concat!(
        "SELECT f.id, f.path, f.size_bytes, f.sha256, f.state, fl.page_count,
                fl.thumbnail_sha256 IS NOT NULL AS has_thumbnail, ",
        staves!(),
        " AS staves, f.message, EXISTS (SELECT 1 FROM import_items i WHERE i.file_id = f.id) AS matched
         FROM import_files f LEFT JOIN files fl ON fl.id = f.file_id
         WHERE f.batch_id = ?1 AND (?2 IS NULL OR f.id = ?2) ORDER BY f.path"
    ))
    .bind(batch_id)
    .bind(file_id)
    .fetch_all(&app.db.read)
    .await?)
}

/// Instruments, aliases, pieces and groups for the review.
struct Context {
    instruments: Vec<Instrument>,
    aliases: Vec<Alias>,
    scores: Vec<ScoreRef>,
    groups: Vec<Group>,
}

impl Context {
    async fn load(app: &App, user: &AuthUser) -> ApiResult<Self> {
        let instruments: Vec<(Id, String, Option<String>)> =
            sqlx::query_as("SELECT id, name, family FROM instruments").fetch_all(&app.db.read).await?;
        let aliases: Vec<(String, Id)> =
            sqlx::query_as("SELECT alias, instrument_id FROM instrument_aliases").fetch_all(&app.db.read).await?;
        // The pieces an import may be matched with: the ones this user can see (librarians: all).
        let scores: Vec<(Id, String, Option<String>, Option<String>)> = sqlx::query_as(concat!(
            "SELECT s.id, s.title, s.composer, s.archive_number FROM scores s
             WHERE ?2 = 1 OR EXISTS (SELECT 1 FROM parts p WHERE p.score_id = s.id AND ",
            part_visible!(),
            ")"
        ))
        .bind(user.id)
        .bind(librarian_flag(user))
        .fetch_all(&app.db.read)
        .await?;
        let groups: Vec<(Id, String, String, String)> = sqlx::query_as(
            "SELECT g.id, g.name,
                    (SELECT json_group_array(instrument_id) FROM group_instrument_grants WHERE group_id = g.id),
                    (SELECT json_group_array(score_id) FROM group_score_grants WHERE group_id = g.id)
             FROM groups g",
        )
        .fetch_all(&app.db.read)
        .await?;
        Ok(Self {
            instruments: instruments.into_iter().map(|(id, name, family)| Instrument { id, name, family }).collect(),
            aliases: aliases.into_iter().map(|(alias, instrument_id)| Alias { alias, instrument_id }).collect(),
            scores: scores
                .into_iter()
                .map(|(id, title, composer, archive_number)| ScoreRef { id, title, composer, archive_number })
                .collect(),
            groups: groups
                .into_iter()
                .map(|(id, name, instruments, scores)| Group {
                    id,
                    name,
                    member_ids: vec![],
                    instrument_ids: serde_json::from_str(&instruments).unwrap_or_default(),
                    score_ids: serde_json::from_str(&scores).unwrap_or_default(),
                })
                .collect(),
        })
    }

    fn library(&self) -> Library<'_> {
        Library { instruments: &self.instruments, aliases: &self.aliases, scores: &self.scores, groups: &self.groups }
    }
}

#[derive(sqlx::FromRow)]
struct ItemRow {
    id: Id,
    kind: String,
    piece_id: Option<Id>,
    file_id: Option<Id>,
    proposal: String,
    overrides: String,
    state: String,
    score_id: Option<Id>,
    part_id: Option<Id>,
}

fn item_state(s: &str) -> ItemState {
    if s == "committed" { ItemState::Committed } else { ItemState::Pending }
}

fn parse<T: serde::de::DeserializeOwned>(json: &str) -> ApiResult<T> {
    serde_json::from_str(json).map_err(AppError::internal)
}

/// The review rows of a batch (or of some of its pieces).
async fn review(
    app: &App,
    user: &AuthUser,
    batch_id: Id,
    only: Option<&[Id]>,
) -> ApiResult<(Vec<ReviewPiece>, Vec<FileRow>)> {
    let files = staged_files(app, batch_id, None).await?;
    let by_id: HashMap<Id, &FileRow> = files.iter().map(|f| (f.id, f)).collect();
    let items: Vec<ItemRow> = sqlx::query_as(
        "SELECT id, kind, piece_id, file_id, proposal, overrides, state, score_id, part_id
         FROM import_items WHERE batch_id = ?1 ORDER BY position, id",
    )
    .bind(batch_id)
    .fetch_all(&app.db.read)
    .await?;
    let ctx = Context::load(app, user).await?;
    let lib = ctx.library();
    let mut parts_of: HashMap<Id, Vec<&ItemRow>> = HashMap::new();
    for i in items.iter().filter(|i| i.kind == "part") {
        if let Some(p) = i.piece_id {
            parts_of.entry(p).or_default().push(i);
        }
    }
    let mut out = Vec::new();
    for piece in items.iter().filter(|i| i.kind == "piece" && only.is_none_or(|o| o.contains(&i.id))) {
        let guess: PieceGuess = parse(&piece.proposal)?;
        let over: PieceOverrides = parse(&piece.overrides)?;
        let mut guesses: Vec<(&ItemRow, PartGuess, PartOverrides, StagedFile)> = Vec::new();
        for part in parts_of.get(&piece.id).into_iter().flatten() {
            let Some(file) = part.file_id.and_then(|f| by_id.get(&f)) else { continue };
            // Content that turned out to be in the library meanwhile is not imported.
            if file.state == "duplicate" {
                continue;
            }
            let staged = StagedFile::from(FileRow {
                id: file.id,
                path: file.path.clone(),
                size_bytes: file.size_bytes,
                sha256: file.sha256.clone(),
                state: file.state.clone(),
                page_count: file.page_count,
                has_thumbnail: file.has_thumbnail,
                staves: file.staves,
                message: file.message.clone(),
                matched: true,
            });
            guesses.push((part, parse(&part.proposal)?, parse(&part.overrides)?, staged));
        }
        let inputs = guesses
            .iter()
            .map(|(row, guess, over, file)| ReviewPartInput {
                id: row.id,
                guess,
                overrides: over,
                file: file.clone(),
                state: item_state(&row.state),
                part_id: row.part_id,
            })
            .collect();
        out.push(review_piece(piece.id, item_state(&piece.state), &guess, &over, piece.score_id, inputs, &lib));
    }
    Ok((out, files))
}

pub async fn get_review(
    State(app): State<AppState>,
    user: AuthUser,
    Path(id): Path<Id>,
) -> ApiResult<Json<ImportReview>> {
    user.require_librarian()?;
    let row = batch_row(&app, id).await?;
    let (pieces, files) = review(&app, &user, id, None).await?;
    let (mut unmatched, mut duplicates) = (Vec::new(), Vec::new());
    for f in files {
        if f.state == "duplicate" {
            duplicates.push(StagedFile::from(f));
        } else if !f.matched {
            unmatched.push(StagedFile::from(f));
        }
    }
    Ok(Json(ImportReview { batch: batch_info(&app, &row).await?, pieces, unmatched, duplicates }))
}

/// Receives the content of a registered file. The hash must be the one the
/// browser announced; the content must be a PDF.
pub async fn upload_content(
    State(app): State<AppState>,
    user: AuthUser,
    Path(id): Path<Id>,
    body: Body,
) -> ApiResult<Json<StagedFile>> {
    user.require_librarian()?;
    let (batch_id, expected, state): (Id, String, String) =
        sqlx::query_as("SELECT batch_id, sha256, state FROM import_files WHERE id = ?1")
            .bind(id)
            .fetch_optional(&app.db.read)
            .await?
            .ok_or(AppError::NotFound)?;
    if state == "pending" || state == "failed" {
        let pending = app.blobs.receive(body, app.settings.max_upload_bytes).await.map_err(|e| match e {
            UploadError::TooLarge(limit) => AppError::PayloadTooLarge(limit),
            UploadError::Body(e) => AppError::BadRequest(format!("upload interrupted: {e}")),
            UploadError::Io(e) => AppError::internal(e),
        })?;
        let result = async {
            if pending.sha256 != expected {
                return Err(AppError::Unprocessable(
                    "the file changed after it was checked; select the folder again".into(),
                ));
            }
            let mime = library::sniff_pending(&pending).await?;
            library::store_file(&app, &pending, mime).await
        }
        .await;
        match result {
            Ok(file_id) => {
                sqlx::query(
                    "UPDATE import_files SET file_id = ?2, message = NULL,
                       state = CASE WHEN EXISTS (SELECT 1 FROM part_files WHERE file_id = ?2) THEN 'duplicate' ELSE 'uploaded' END
                     WHERE id = ?1",
                )
                .bind(id)
                .bind(file_id)
                .execute(&app.db.write)
                .await?;
            }
            Err(e) => {
                app.blobs.discard(&pending).await;
                sqlx::query("UPDATE import_files SET state = 'failed', message = ?2 WHERE id = ?1")
                    .bind(id)
                    .bind(e.to_string())
                    .execute(&app.db.write)
                    .await?;
                return Err(e);
            }
        }
    }
    let row = staged_files(&app, batch_id, Some(id)).await?.pop().ok_or(AppError::NotFound)?;
    Ok(Json(row.into()))
}

/// `(pdf sha, thumbnail sha)` of a staged file: librarians only, through its
/// batch; staged files are not part of the library (a-import-staging).
async fn staged_blob(app: &App, user: &AuthUser, id: Id) -> ApiResult<(String, Option<String>)> {
    user.require_librarian()?;
    sqlx::query_as(
        "SELECT fl.sha256, fl.thumbnail_sha256 FROM import_files f JOIN files fl ON fl.id = f.file_id WHERE f.id = ?1",
    )
    .bind(id)
    .fetch_optional(&app.db.read)
    .await?
    .ok_or(AppError::NotFound)
}

pub async fn get_staged_file(
    State(app): State<AppState>,
    user: AuthUser,
    Path(id): Path<Id>,
    req: Request,
) -> ApiResult<Response> {
    let (sha, _) = staged_blob(&app, &user, id).await?;
    serve_blob(&app, &sha, crate::processors::PDF_MIME, req).await
}

pub async fn get_staged_thumbnail(
    State(app): State<AppState>,
    user: AuthUser,
    Path(id): Path<Id>,
    req: Request,
) -> ApiResult<Response> {
    let (_, thumbnail) = staged_blob(&app, &user, id).await?;
    serve_blob(&app, &thumbnail.ok_or(AppError::NotFound)?, "image/png", req).await
}

/// The librarian's changes to pieces and parts (also several at once).
/// Answers with the changed pieces' review rows.
pub async fn update_items(
    State(app): State<AppState>,
    user: AuthUser,
    Json(input): Json<UpdateImportItems>,
) -> ApiResult<Json<Vec<ReviewPiece>>> {
    user.require_librarian()?;
    if input.ids.is_empty() || input.ids.len() > MAX_ITEMS {
        return Err(AppError::BadRequest(format!("1 to {MAX_ITEMS} items")));
    }
    let rows: Vec<(Id, Id, String, Option<Id>, String, String)> = sqlx::query_as(
        "SELECT id, batch_id, kind, piece_id, overrides, state FROM import_items WHERE id IN (SELECT value FROM json_each(?1))",
    )
    .bind(json_ids(&input.ids))
    .fetch_all(&app.db.read)
    .await?;
    let batch_id = match rows.first() {
        Some(r) => r.1,
        None => return Err(AppError::NotFound),
    };
    if rows.len() != input.ids.iter().collect::<HashSet<_>>().len() || rows.iter().any(|r| r.1 != batch_id) {
        return Err(AppError::BadRequest("unknown items, or items of several batches".into()));
    }
    if rows.iter().any(|r| r.5 == "committed") {
        return Err(AppError::Conflict("already imported; undo the batch to change it".into()));
    }
    // Validate the changes before storing them.
    if let Some(PieceOverrides { target: Some(Target::Existing(score)), .. }) = &input.piece {
        let exists: Option<i64> =
            sqlx::query_scalar("SELECT 1 FROM scores WHERE id = ?1").bind(score).fetch_optional(&app.db.read).await?;
        if exists.is_none() {
            return Err(AppError::BadRequest("unknown piece".into()));
        }
    }
    let mut part_changes = input.part.clone();
    if let Some(p) = &mut part_changes {
        if let Some(ids) = &p.instrument_ids {
            let known: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM instruments WHERE id IN (SELECT value FROM json_each(?1))")
                    .bind(json_ids(ids))
                    .fetch_one(&app.db.read)
                    .await?;
            if known as usize != ids.iter().collect::<HashSet<_>>().len() || ids.len() > 20 {
                return Err(AppError::BadRequest("unknown instruments (at most 20 per part)".into()));
            }
        }
        if let Some(k) = p.key.as_ref().map(|k| k.trim().to_owned()).filter(|k| !k.is_empty()) {
            let code = naming::normalize_key(&k).ok_or_else(|| AppError::BadRequest(format!("unknown key \"{k}\"")))?;
            p.key = Some(code.to_owned());
        }
        if let Some(v) = &p.voices {
            let v = naming::clean_voices(v);
            if v.len() > naming::MAX_VOICES || v.iter().any(|x| x.chars().count() > naming::MAX_VOICE_CHARS) {
                return Err(AppError::BadRequest(format!(
                    "at most {} voices of at most {} characters",
                    naming::MAX_VOICES,
                    naming::MAX_VOICE_CHARS
                )));
            }
            p.voices = Some(v);
        }
        if p.label.as_ref().is_some_and(|v| v.trim().chars().count() > 120) {
            return Err(AppError::BadRequest("custom name at most 120 characters".into()));
        }
    }
    if let Some(p) = &input.piece {
        let too_long = [(&p.title, 200), (&p.composer, 200), (&p.arranger, 200), (&p.genre, 60), (&p.number, 40)]
            .iter()
            .any(|(v, max)| v.as_ref().is_some_and(|v| v.trim().chars().count() > *max));
        if too_long {
            return Err(AppError::BadRequest("value too long".into()));
        }
    }
    let mut tx = app.db.write.begin().await?;
    let mut pieces: Vec<Id> = Vec::new();
    for (id, _, kind, piece_id, overrides, _) in &rows {
        let json = if kind == "piece" {
            pieces.push(*id);
            let mut o: PieceOverrides = if input.reset { PieceOverrides::default() } else { parse(overrides)? };
            if let Some(c) = &input.piece {
                o.merge(c);
            }
            serde_json::to_string(&o).map_err(AppError::internal)?
        } else {
            pieces.extend(piece_id);
            let mut o: PartOverrides = if input.reset { PartOverrides::default() } else { parse(overrides)? };
            if let Some(c) = &part_changes {
                o.merge(c);
            }
            serde_json::to_string(&o).map_err(AppError::internal)?
        };
        sqlx::query("UPDATE import_items SET overrides = ?2 WHERE id = ?1")
            .bind(id)
            .bind(json)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    pieces.sort_unstable();
    pieces.dedup();
    Ok(Json(review(&app, &user, batch_id, Some(&pieces)).await?.0))
}

/// Creates the chosen pieces (or every green one), each in its own short
/// write transaction (d-sqlite), with the same validation as the piece and
/// part forms.
pub async fn commit(
    State(app): State<AppState>,
    user: AuthUser,
    Path(batch_id): Path<Id>,
    Json(input): Json<CommitImport>,
) -> ApiResult<Json<CommittedImport>> {
    user.require_librarian()?;
    batch_row(&app, batch_id).await?;
    let (pieces, _) = review(&app, &user, batch_id, input.piece_ids.as_deref()).await?;
    let mut result = CommittedImport::default();
    if let Some(ids) = &input.piece_ids {
        for id in ids.iter().filter(|id| !pieces.iter().any(|p| p.id == **id)) {
            result.errors.push(ImportItemError { id: *id, message: "not a piece of this batch".into() });
        }
    }
    for piece in &pieces {
        let chosen = match input.piece_ids {
            Some(_) => {
                if !piece.can_commit() {
                    let why = piece.blocking_note().map(|n| n.message()).unwrap_or_else(|| "not ready".into());
                    let why = if piece.state == ItemState::Committed { "already imported".into() } else { why };
                    result.errors.push(ImportItemError { id: piece.id, message: why });
                    continue;
                }
                true
            }
            None => piece.is_green(),
        };
        if !chosen {
            continue;
        }
        match commit_piece(&app, piece).await {
            Ok(()) => result.committed.push(piece.id),
            Err(e) => result.errors.push(ImportItemError { id: piece.id, message: e.to_string() }),
        }
    }
    Ok(Json(result))
}

async fn commit_piece(app: &App, piece: &ReviewPiece) -> ApiResult<()> {
    let mut tx = app.db.write.begin().await?;
    let state: Option<String> = sqlx::query_scalar("SELECT state FROM import_items WHERE id = ?1 AND kind = 'piece'")
        .bind(piece.id)
        .fetch_optional(&mut *tx)
        .await?;
    if state.as_deref() != Some("pending") {
        return Err(AppError::Conflict("already imported".into()));
    }
    let (score_id, created) = match piece.effective_target() {
        Target::Existing(id) => (id, false),
        Target::New => {
            let input = ScoreInput {
                title: piece.title.value.clone().unwrap_or_default(),
                composer: piece.composer.value.clone(),
                arranger: piece.arranger.value.clone(),
                genre: piece.genre.value.clone(),
                archive_number: piece.number.value.clone(),
                ..ScoreInput::default()
            };
            (library::insert_score(&mut tx, library::clean_score(&input)?).await?, true)
        }
    };
    let mut part_ids = Vec::new();
    let now = now();
    for part in piece.parts.iter().filter(|p| !p.skip && p.state == ItemState::Pending) {
        let file_id: Option<Id> =
            sqlx::query_scalar("SELECT file_id FROM import_files WHERE id = ?1 AND state = 'uploaded'")
                .bind(part.file.id)
                .fetch_optional(&mut *tx)
                .await?
                .flatten();
        let file_id = file_id.ok_or_else(|| AppError::Conflict(format!("{} is not uploaded", part.file.path)))?;
        let input = PartInput {
            label: part.label.clone(),
            instrument_ids: part.instruments.iter().filter_map(|i| i.instrument_id).collect(),
            voices: part.voices.values.clone(),
            transposition: part.key.value.clone(),
            notes: None,
        };
        let id = library::insert_part(&mut tx, score_id, library::clean_part(&input, None)?).await?;
        library::add_version(&mut tx, id, file_id).await?;
        sqlx::query(
            "UPDATE import_items SET state = 'committed', part_id = ?2, score_id = ?3, committed_at = ?4 WHERE id = ?1",
        )
        .bind(part.id)
        .bind(id)
        .bind(score_id)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        part_ids.push(id);
    }
    part_index::reindex(&mut tx, Some(&part_ids)).await?;
    sqlx::query(
        "UPDATE import_items SET state = 'committed', score_id = ?2, created_score = ?3, committed_at = ?4 WHERE id = ?1",
    )
    .bind(piece.id)
    .bind(score_id)
    .bind(created)
    .bind(now)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

/// Removes what the batch created: its parts, and the pieces it created.
/// Refused (409, nothing removed) if annotations exist on an imported part,
/// if an imported piece got other parts since, or is in a programme.
pub async fn undo(
    State(app): State<AppState>,
    user: AuthUser,
    Path(batch_id): Path<Id>,
) -> ApiResult<Json<UndoneImport>> {
    user.require_librarian()?;
    batch_row(&app, batch_id).await?;
    let parts: Vec<Id> = sqlx::query_scalar(
        "SELECT part_id FROM import_items WHERE batch_id = ?1 AND kind = 'part' AND state = 'committed' AND part_id IS NOT NULL",
    )
    .bind(batch_id)
    .fetch_all(&app.db.read)
    .await?;
    let scores: Vec<Id> = sqlx::query_scalar(
        "SELECT score_id FROM import_items
         WHERE batch_id = ?1 AND kind = 'piece' AND state = 'committed' AND created_score = 1 AND score_id IS NOT NULL",
    )
    .bind(batch_id)
    .fetch_all(&app.db.read)
    .await?;
    let (parts_json, scores_json) = (json_ids(&parts), json_ids(&scores));
    let annotated: Vec<String> = sqlx::query_scalar(concat!(
        "SELECT DISTINCT s.title FROM parts p JOIN scores s ON s.id = p.score_id
         WHERE p.id IN (SELECT value FROM json_each(?3))
           AND EXISTS (SELECT 1 FROM annotation_sets a WHERE a.part_id = p.id) AND ",
        part_visible!(),
        " ORDER BY s.title"
    ))
    .bind(user.id)
    .bind(librarian_flag(&user))
    .bind(&parts_json)
    .fetch_all(&app.db.read)
    .await?;
    if !annotated.is_empty() {
        return Err(AppError::Conflict(format!(
            "cannot undo: musicians have annotated imported parts of {}",
            annotated.join(", ")
        )));
    }
    let changed: Vec<String> = sqlx::query_scalar(concat!(
        "SELECT DISTINCT s.title FROM scores s JOIN parts p ON p.score_id = s.id
         WHERE s.id IN (SELECT value FROM json_each(?4)) AND p.id NOT IN (SELECT value FROM json_each(?3)) AND ",
        part_visible!(),
        " UNION SELECT s.title FROM scores s JOIN programme_items pi ON pi.score_id = s.id
         WHERE s.id IN (SELECT value FROM json_each(?4)) ORDER BY 1"
    ))
    .bind(user.id)
    .bind(librarian_flag(&user))
    .bind(&parts_json)
    .bind(&scores_json)
    .fetch_all(&app.db.read)
    .await?;
    if !changed.is_empty() {
        return Err(AppError::Conflict(format!(
            "cannot undo: these imported pieces got other parts or are in a programme since: {}",
            changed.join(", ")
        )));
    }
    let mut tx = app.db.write.begin().await?;
    let removed_parts = sqlx::query("DELETE FROM parts WHERE id IN (SELECT value FROM json_each(?1))")
        .bind(&parts_json)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    let removed_scores = sqlx::query("DELETE FROM scores WHERE id IN (SELECT value FROM json_each(?1))")
        .bind(&scores_json)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    sqlx::query(
        "UPDATE import_items SET state = 'pending', score_id = NULL, part_id = NULL, created_score = 0, committed_at = NULL
         WHERE batch_id = ?1 AND state = 'committed'",
    )
    .bind(batch_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Json(UndoneImport { pieces: removed_scores as i64, parts: removed_parts as i64 }))
}

pub async fn list_aliases(State(app): State<AppState>, user: AuthUser) -> ApiResult<Json<Vec<InstrumentAlias>>> {
    user.require_librarian()?;
    let rows: Vec<(String, Id, String)> = sqlx::query_as(
        "SELECT a.alias, a.instrument_id, i.name FROM instrument_aliases a JOIN instruments i ON i.id = a.instrument_id
         ORDER BY a.alias",
    )
    .fetch_all(&app.db.read)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|(alias, instrument_id, instrument_name)| InstrumentAlias { alias, instrument_id, instrument_name })
            .collect(),
    ))
}

/// Maps a file-name word to an instrument (replaces an earlier mapping).
/// Applies at once to every open import (the review is resolved on read).
pub async fn put_alias(
    State(app): State<AppState>,
    user: AuthUser,
    Json(input): Json<InstrumentAliasInput>,
) -> ApiResult<Json<InstrumentAlias>> {
    user.require_librarian()?;
    let alias = alias_key(&input.alias);
    if alias.is_empty() || alias.chars().count() > 80 {
        return Err(AppError::BadRequest("alias must have 1 to 80 characters".into()));
    }
    let name: String = sqlx::query_scalar("SELECT name FROM instruments WHERE id = ?1")
        .bind(input.instrument_id)
        .fetch_optional(&app.db.read)
        .await?
        .ok_or_else(|| AppError::BadRequest("unknown instrument".into()))?;
    sqlx::query(
        "INSERT INTO instrument_aliases (alias, instrument_id, created_by, created_at) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT (alias) DO UPDATE SET instrument_id = ?2, created_by = ?3, created_at = ?4",
    )
    .bind(&alias)
    .bind(input.instrument_id)
    .bind(user.id)
    .bind(now())
    .execute(&app.db.write)
    .await?;
    Ok(Json(InstrumentAlias { alias, instrument_id: input.instrument_id, instrument_name: name }))
}

#[derive(Deserialize)]
pub struct AliasQuery {
    alias: String,
}

pub async fn delete_alias(
    State(app): State<AppState>,
    user: AuthUser,
    Query(q): Query<AliasQuery>,
) -> ApiResult<StatusCode> {
    user.require_librarian()?;
    let done = sqlx::query("DELETE FROM instrument_aliases WHERE alias = ?1")
        .bind(alias_key(&q.alias))
        .execute(&app.db.write)
        .await?;
    if done.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}
