//! Scores, parts, uploads and file versions.

use crate::{
    access::{ensure_part_readable, librarian_flag},
    auth::AuthUser,
    blobs::UploadError,
    db::{json_ids, now},
    error::{ApiResult, AppError, optional, required},
    part_index, part_visible,
    processors::{self, Processed},
    state::{App, AppState},
};
use axum::{
    Json,
    body::Body,
    extract::{Path, Query, State},
    http::StatusCode,
};
use musicus_api::{FileVersion, Id, Part, PartInput, Score, ScoreInput, naming};
use serde::Deserialize;
use std::collections::HashMap;
use tokio::io::AsyncReadExt;

#[derive(sqlx::FromRow)]
pub struct ScoreRow {
    id: Id,
    title: String,
    subtitle: Option<String>,
    composer: Option<String>,
    arranger: Option<String>,
    genre: Option<String>,
    tags: String,
    notes: Option<String>,
}

#[derive(sqlx::FromRow)]
struct PartRow {
    id: Id,
    score_id: Id,
    label: Option<String>,
    voice: Option<String>,
    transposition: Option<String>,
    notes: Option<String>,
    instruments: String,
    instrument_names: String,
    version: Option<i64>,
    sha256: Option<String>,
    mime: Option<String>,
    size_bytes: Option<i64>,
    page_count: Option<i64>,
    has_thumbnail: Option<bool>,
    uploaded_at: Option<i64>,
}

impl From<PartRow> for Part {
    fn from(r: PartRow) -> Self {
        let file = match (r.version, r.sha256, r.mime) {
            (Some(version), Some(sha256), Some(mime)) => Some(FileVersion {
                version,
                sha256,
                mime,
                size_bytes: r.size_bytes.unwrap_or(0),
                page_count: r.page_count,
                has_thumbnail: r.has_thumbnail.unwrap_or(false),
                uploaded_at: r.uploaded_at.unwrap_or(0),
            }),
            _ => None,
        };
        let mut instrument_ids: Vec<Id> = serde_json::from_str(&r.instruments).unwrap_or_default();
        instrument_ids.sort_unstable();
        let names: Vec<String> = serde_json::from_str(&r.instrument_names).unwrap_or_default();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        let display_name =
            naming::part_name(r.label.as_deref(), &names, r.transposition.as_deref(), r.voice.as_deref());
        Part {
            id: r.id,
            score_id: r.score_id,
            label: r.label,
            display_name,
            instrument_ids,
            voice: r.voice,
            transposition: r.transposition,
            notes: r.notes,
            file,
        }
    }
}

/// Parts the user may read, restricted to some scores or to one part.
pub async fn visible_parts(
    app: &App,
    user: &AuthUser,
    score_ids: Option<&[Id]>,
    part_id: Option<Id>,
) -> ApiResult<Vec<Part>> {
    let rows: Vec<PartRow> = sqlx::query_as(concat!(
        "SELECT p.id, p.score_id, p.label, p.voice, p.transposition, p.notes,
                (SELECT json_group_array(instrument_id) FROM part_instruments WHERE part_id = p.id) AS instruments,
                (SELECT json_group_array(i.name) FROM part_instruments pi JOIN instruments i ON i.id = pi.instrument_id
                 WHERE pi.part_id = p.id) AS instrument_names,
                pf.version, f.sha256, f.mime, f.size_bytes, f.page_count,
                f.thumbnail_sha256 IS NOT NULL AS has_thumbnail, pf.uploaded_at
         FROM parts p
         LEFT JOIN part_files pf ON pf.part_id = p.id
              AND pf.version = (SELECT MAX(version) FROM part_files x WHERE x.part_id = p.id)
         LEFT JOIN files f ON f.id = pf.file_id
         WHERE (?3 IS NULL OR p.score_id IN (SELECT value FROM json_each(?3)))
           AND (?4 IS NULL OR p.id = ?4) AND ",
        part_visible!(),
        " ORDER BY p.score_id, p.id"
    ))
    .bind(user.id)
    .bind(librarian_flag(user))
    .bind(score_ids.map(json_ids))
    .bind(part_id)
    .fetch_all(&app.db.read)
    .await?;
    let mut parts: Vec<Part> = rows.into_iter().map(Part::from).collect();
    // Within a score by shown name, numbers by value ("Trumpet in B♭ 2" before "… 10").
    parts.sort_by(|a, b| {
        a.score_id.cmp(&b.score_id).then_with(|| naming::natural_cmp(&a.display_name, &b.display_name))
    });
    Ok(parts)
}

/// Attaches the user's visible parts to score rows.
pub async fn with_parts(app: &App, user: &AuthUser, rows: Vec<ScoreRow>) -> ApiResult<Vec<Score>> {
    let ids: Vec<Id> = rows.iter().map(|r| r.id).collect();
    let mut parts: HashMap<Id, Vec<Part>> = HashMap::new();
    for part in visible_parts(app, user, Some(&ids), None).await? {
        parts.entry(part.score_id).or_default().push(part);
    }
    Ok(rows
        .into_iter()
        .map(|r| Score {
            parts: parts.remove(&r.id).unwrap_or_default(),
            id: r.id,
            title: r.title,
            subtitle: r.subtitle,
            composer: r.composer,
            arranger: r.arranger,
            genre: r.genre,
            tags: serde_json::from_str(&r.tags).unwrap_or_default(),
            notes: r.notes,
        })
        .collect())
}

/// Scores by id, in the given order, that the user may see: librarians see
/// all, members those with at least one readable part.
pub async fn scores_by_ids(app: &App, user: &AuthUser, ids: &[Id]) -> ApiResult<Vec<Score>> {
    let rows: Vec<ScoreRow> = sqlx::query_as(concat!(
        "SELECT s.id, s.title, s.subtitle, s.composer, s.arranger, s.genre, s.tags, s.notes FROM scores s
         WHERE s.id IN (SELECT value FROM json_each(?3))
           AND (?2 = 1 OR EXISTS (SELECT 1 FROM parts p WHERE p.score_id = s.id AND ",
        part_visible!(),
        "))"
    ))
    .bind(user.id)
    .bind(librarian_flag(user))
    .bind(json_ids(ids))
    .fetch_all(&app.db.read)
    .await?;
    let scores: HashMap<Id, Score> = with_parts(app, user, rows).await?.into_iter().map(|s| (s.id, s)).collect();
    // A programme may list a piece twice (an encore), hence clone.
    Ok(ids.iter().filter_map(|id| scores.get(id).cloned()).collect())
}

/// Turns user input into FTS5 prefix queries, one per word, as a JSON
/// array (bound as one parameter, expanded with `json_each`). Every word
/// must match the score's metadata or the shown name of one of the user's
/// parts of it.
fn fts_terms(q: &str) -> Option<String> {
    let mut terms: Vec<String> = Vec::new();
    for w in q.split_whitespace().take(10) {
        let w: String = w.chars().filter(|c| c.is_alphanumeric()).collect();
        let term = format!("\"{w}\"*");
        if !w.is_empty() && !terms.contains(&term) {
            terms.push(term);
        }
    }
    (!terms.is_empty()).then(|| serde_json::to_string(&terms).expect("terms serialise"))
}

#[derive(Deserialize)]
pub struct ScoreQuery {
    #[serde(default)]
    q: Option<String>,
    #[serde(default)]
    genre: Option<String>,
    #[serde(default)]
    tag: Option<String>,
}

pub async fn list_scores(
    State(app): State<AppState>,
    user: AuthUser,
    Query(query): Query<ScoreQuery>,
) -> ApiResult<Json<Vec<Score>>> {
    let rows: Vec<ScoreRow> = sqlx::query_as(concat!(
        "SELECT s.id, s.title, s.subtitle, s.composer, s.arranger, s.genre, s.tags, s.notes FROM scores s
         WHERE (?3 IS NULL OR s.id IN (
                  SELECT sid FROM (
                      SELECT t.value AS term, score_fts.rowid AS sid FROM json_each(?3) t CROSS JOIN score_fts
                      WHERE score_fts MATCH t.value
                      UNION
                      SELECT t.value, p.score_id FROM json_each(?3) t CROSS JOIN part_fts JOIN parts p ON p.id = part_fts.rowid
                      WHERE part_fts MATCH t.value AND ",
        part_visible!(),
        ") GROUP BY sid HAVING COUNT(*) = json_array_length(?3)))
           AND (?4 IS NULL OR s.genre = ?4 COLLATE NOCASE)
           AND (?5 IS NULL OR EXISTS (SELECT 1 FROM json_each(s.tags) WHERE value = ?5 COLLATE NOCASE))
           AND (?2 = 1 OR EXISTS (SELECT 1 FROM parts p WHERE p.score_id = s.id AND ",
        part_visible!(),
        ")) ORDER BY s.title COLLATE NOCASE LIMIT 1000"
    ))
    .bind(user.id)
    .bind(librarian_flag(&user))
    .bind(query.q.as_deref().and_then(fts_terms))
    .bind(query.genre.filter(|g| !g.trim().is_empty()))
    .bind(query.tag.filter(|t| !t.trim().is_empty()))
    .fetch_all(&app.db.read)
    .await?;
    Ok(Json(with_parts(&app, &user, rows).await?))
}

/// Every tag used in the library, for suggestions while tagging (a-tag-suggest):
/// librarians get all tags, others only those of scores they can see, so
/// no tag of a hidden score leaks. Case-insensitive duplicates are merged
/// (first spelling wins); sorted case-insensitively.
pub async fn list_tags(State(app): State<AppState>, user: AuthUser) -> ApiResult<Json<Vec<String>>> {
    let raw: Vec<String> = sqlx::query_scalar(concat!(
        "SELECT DISTINCT t.value FROM scores s, json_each(s.tags) t
         WHERE ?2 = 1 OR EXISTS (SELECT 1 FROM parts p WHERE p.score_id = s.id AND ",
        part_visible!(),
        ")"
    ))
    .bind(user.id)
    .bind(librarian_flag(&user))
    .fetch_all(&app.db.read)
    .await?;
    let mut tags: Vec<String> = Vec::new();
    for t in raw {
        if !tags.iter().any(|x| x.to_lowercase() == t.to_lowercase()) {
            tags.push(t);
        }
    }
    tags.sort_by(|a, b| naming::natural_cmp(a, b));
    Ok(Json(tags))
}

pub async fn get_score(State(app): State<AppState>, user: AuthUser, Path(id): Path<Id>) -> ApiResult<Json<Score>> {
    scores_by_ids(&app, &user, &[id]).await?.pop().map(Json).ok_or(AppError::NotFound)
}

struct CleanScore {
    title: String,
    subtitle: Option<String>,
    composer: Option<String>,
    arranger: Option<String>,
    genre: Option<String>,
    tags: String,
    notes: Option<String>,
}

fn clean_score(input: &ScoreInput) -> ApiResult<CleanScore> {
    let mut tags: Vec<String> = Vec::new();
    for tag in &input.tags {
        let tag = required("tag", tag, 40)?;
        if !tags.iter().any(|t| t.eq_ignore_ascii_case(&tag)) {
            tags.push(tag);
        }
    }
    if tags.len() > 30 {
        return Err(AppError::BadRequest("at most 30 tags".into()));
    }
    Ok(CleanScore {
        title: required("title", &input.title, 200)?,
        subtitle: optional("subtitle", input.subtitle.as_deref(), 200)?,
        composer: optional("composer", input.composer.as_deref(), 200)?,
        arranger: optional("arranger", input.arranger.as_deref(), 200)?,
        genre: optional("genre", input.genre.as_deref(), 60)?,
        tags: serde_json::to_string(&tags).expect("tags serialise"),
        notes: optional("notes", input.notes.as_deref(), 4000)?,
    })
}

pub async fn create_score(
    State(app): State<AppState>,
    user: AuthUser,
    Json(input): Json<ScoreInput>,
) -> ApiResult<Json<Score>> {
    user.require_librarian()?;
    let s = clean_score(&input)?;
    let now = now();
    let id: Id = sqlx::query_scalar(
        "INSERT INTO scores (title, subtitle, composer, arranger, genre, tags, notes, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8) RETURNING id",
    )
    .bind(s.title)
    .bind(s.subtitle)
    .bind(s.composer)
    .bind(s.arranger)
    .bind(s.genre)
    .bind(s.tags)
    .bind(s.notes)
    .bind(now)
    .fetch_one(&app.db.write)
    .await?;
    get_score(State(app), user, Path(id)).await
}

pub async fn update_score(
    State(app): State<AppState>,
    user: AuthUser,
    Path(id): Path<Id>,
    Json(input): Json<ScoreInput>,
) -> ApiResult<Json<Score>> {
    user.require_librarian()?;
    let s = clean_score(&input)?;
    let done = sqlx::query(
        "UPDATE scores SET title = ?2, subtitle = ?3, composer = ?4, arranger = ?5, genre = ?6, tags = ?7,
                           notes = ?8, updated_at = ?9 WHERE id = ?1",
    )
    .bind(id)
    .bind(s.title)
    .bind(s.subtitle)
    .bind(s.composer)
    .bind(s.arranger)
    .bind(s.genre)
    .bind(s.tags)
    .bind(s.notes)
    .bind(now())
    .execute(&app.db.write)
    .await?;
    if done.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    get_score(State(app), user, Path(id)).await
}

/// Deletes the score with all parts and the annotations drawn on them.
/// Files stay in the blob store.
pub async fn delete_score(State(app): State<AppState>, user: AuthUser, Path(id): Path<Id>) -> ApiResult<StatusCode> {
    user.require_librarian()?;
    let done = sqlx::query("DELETE FROM scores WHERE id = ?1").bind(id).execute(&app.db.write).await?;
    if done.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

struct CleanPart {
    label: Option<String>,
    voice: Option<String>,
    transposition: Option<String>,
    notes: Option<String>,
    instruments: String,
}

/// Validates a part. The key must be one of `naming::KEYS` (common
/// spellings are normalised); `stored_key` is the part's current key, which
/// may be kept even if it is an older free-text value (a-part-names).
fn clean_part(input: &PartInput, stored_key: Option<&str>) -> ApiResult<CleanPart> {
    if input.instrument_ids.len() > 20 {
        return Err(AppError::BadRequest("at most 20 instruments per part".into()));
    }
    let transposition = match input.transposition.as_deref().map(str::trim).filter(|k| !k.is_empty()) {
        None => None,
        Some(k) if Some(k) == stored_key => Some(k.to_owned()),
        Some(k) => match naming::normalize_key(k) {
            Some(code) => Some(code.to_owned()),
            None => {
                let known: Vec<&str> = naming::KEYS.iter().map(|k| k.code).collect();
                return Err(AppError::BadRequest(format!("unknown key \"{k}\"; use one of {}", known.join(", "))));
            }
        },
    };
    Ok(CleanPart {
        label: optional("label", input.label.as_deref(), 120)?,
        voice: optional("voice", input.voice.as_deref(), 20)?,
        transposition,
        notes: optional("notes", input.notes.as_deref(), 2000)?,
        instruments: json_ids(&input.instrument_ids),
    })
}

async fn one_part(app: &App, user: &AuthUser, id: Id) -> ApiResult<Json<Part>> {
    visible_parts(app, user, None, Some(id)).await?.pop().map(Json).ok_or(AppError::NotFound)
}

pub async fn get_part(State(app): State<AppState>, user: AuthUser, Path(id): Path<Id>) -> ApiResult<Json<Part>> {
    one_part(&app, &user, id).await
}

pub async fn create_part(
    State(app): State<AppState>,
    user: AuthUser,
    Path(score_id): Path<Id>,
    Json(input): Json<PartInput>,
) -> ApiResult<Json<Part>> {
    user.require_librarian()?;
    let p = clean_part(&input, None)?;
    let mut tx = app.db.write.begin().await?;
    let exists: Option<i64> =
        sqlx::query_scalar("SELECT 1 FROM scores WHERE id = ?1").bind(score_id).fetch_optional(&mut *tx).await?;
    if exists.is_none() {
        return Err(AppError::NotFound);
    }
    let id: Id = sqlx::query_scalar(
        "INSERT INTO parts (score_id, label, voice, transposition, notes, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6) RETURNING id",
    )
    .bind(score_id)
    .bind(p.label)
    .bind(p.voice)
    .bind(p.transposition)
    .bind(p.notes)
    .bind(now())
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query("INSERT OR IGNORE INTO part_instruments (part_id, instrument_id) SELECT ?1, value FROM json_each(?2)")
        .bind(id)
        .bind(p.instruments)
        .execute(&mut *tx)
        .await?;
    part_index::reindex(&mut tx, Some(&[id])).await?;
    tx.commit().await?;
    one_part(&app, &user, id).await
}

pub async fn update_part(
    State(app): State<AppState>,
    user: AuthUser,
    Path(id): Path<Id>,
    Json(input): Json<PartInput>,
) -> ApiResult<Json<Part>> {
    user.require_librarian()?;
    let mut tx = app.db.write.begin().await?;
    let stored: Option<Option<String>> =
        sqlx::query_scalar("SELECT transposition FROM parts WHERE id = ?1").bind(id).fetch_optional(&mut *tx).await?;
    let Some(stored_key) = stored else { return Err(AppError::NotFound) };
    let p = clean_part(&input, stored_key.as_deref())?;
    let done = sqlx::query("UPDATE parts SET label = ?2, voice = ?3, transposition = ?4, notes = ?5 WHERE id = ?1")
        .bind(id)
        .bind(p.label)
        .bind(p.voice)
        .bind(p.transposition)
        .bind(p.notes)
        .execute(&mut *tx)
        .await?;
    if done.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    sqlx::query("DELETE FROM part_instruments WHERE part_id = ?1").bind(id).execute(&mut *tx).await?;
    sqlx::query("INSERT OR IGNORE INTO part_instruments (part_id, instrument_id) SELECT ?1, value FROM json_each(?2)")
        .bind(id)
        .bind(p.instruments)
        .execute(&mut *tx)
        .await?;
    part_index::reindex(&mut tx, Some(&[id])).await?;
    tx.commit().await?;
    one_part(&app, &user, id).await
}

pub async fn delete_part(State(app): State<AppState>, user: AuthUser, Path(id): Path<Id>) -> ApiResult<StatusCode> {
    user.require_librarian()?;
    let done = sqlx::query("DELETE FROM parts WHERE id = ?1").bind(id).execute(&app.db.write).await?;
    if done.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Uploads a new file version of a part. The body is the raw file.
/// Uploading the current file again creates no new version.
pub async fn upload_file(
    State(app): State<AppState>,
    user: AuthUser,
    Path(part_id): Path<Id>,
    body: Body,
) -> ApiResult<Json<Part>> {
    user.require_librarian()?;
    let exists: Option<i64> =
        sqlx::query_scalar("SELECT 1 FROM parts WHERE id = ?1").bind(part_id).fetch_optional(&app.db.read).await?;
    if exists.is_none() {
        return Err(AppError::NotFound);
    }
    let pending = app.blobs.receive(body, app.settings.max_upload_bytes).await.map_err(|e| match e {
        UploadError::TooLarge(limit) => AppError::PayloadTooLarge(limit),
        UploadError::Body(e) => AppError::BadRequest(format!("upload interrupted: {e}")),
        UploadError::Io(e) => AppError::internal(e),
    })?;
    let result = store_upload(&app, part_id, &pending).await;
    if result.is_err() {
        app.blobs.discard(&pending).await;
    }
    result?;
    one_part(&app, &user, part_id).await
}

async fn store_upload(app: &App, part_id: Id, pending: &crate::blobs::PendingBlob) -> ApiResult<()> {
    let mut head = vec![0u8; 1024];
    let n = tokio::fs::File::open(&pending.tmp_path).await?.read(&mut head).await?;
    let mime =
        processors::sniff_mime(&head[..n]).ok_or(AppError::UnsupportedMediaType("only PDF files are supported"))?;

    let known: Option<Id> = sqlx::query_scalar("SELECT id FROM files WHERE sha256 = ?1")
        .bind(&pending.sha256)
        .fetch_optional(&app.db.read)
        .await?;
    let file_id = match known {
        Some(id) => {
            app.blobs.commit(pending).await?;
            id
        }
        None => {
            let data = tokio::fs::read(&pending.tmp_path).await?;
            // A panic inside the renderer must not take the upload down.
            let processed = tokio::task::spawn_blocking(move || processors::process_pdf(data))
                .await
                .unwrap_or(Processed { mime, page_count: None, thumbnail_png: None });
            let thumbnail = match processed.thumbnail_png {
                Some(png) => Some(app.blobs.put_bytes(&png).await?),
                None => None,
            };
            app.blobs.commit(pending).await?;
            sqlx::query(
                "INSERT INTO files (sha256, mime, size_bytes, page_count, thumbnail_sha256, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT (sha256) DO NOTHING",
            )
            .bind(&pending.sha256)
            .bind(processed.mime)
            .bind(pending.size as i64)
            .bind(processed.page_count)
            .bind(thumbnail)
            .bind(now())
            .execute(&app.db.write)
            .await?;
            sqlx::query_scalar("SELECT id FROM files WHERE sha256 = ?1")
                .bind(&pending.sha256)
                .fetch_one(&app.db.write)
                .await?
        }
    };

    let mut tx = app.db.write.begin().await?;
    let latest: Option<(i64, Id)> =
        sqlx::query_as("SELECT version, file_id FROM part_files WHERE part_id = ?1 ORDER BY version DESC LIMIT 1")
            .bind(part_id)
            .fetch_optional(&mut *tx)
            .await?;
    if latest.is_none_or(|(_, current)| current != file_id) {
        sqlx::query("INSERT INTO part_files (part_id, version, file_id, uploaded_at) VALUES (?1, ?2, ?3, ?4)")
            .bind(part_id)
            .bind(latest.map_or(1, |(v, _)| v + 1))
            .bind(file_id)
            .bind(now())
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// All file versions of a part, newest first.
pub async fn list_versions(
    State(app): State<AppState>,
    user: AuthUser,
    Path(part_id): Path<Id>,
) -> ApiResult<Json<Vec<FileVersion>>> {
    ensure_part_readable(&app, &user, part_id).await?;
    type VersionRow = (i64, String, String, i64, Option<i64>, bool, i64);
    let rows: Vec<VersionRow> = sqlx::query_as(
        "SELECT pf.version, f.sha256, f.mime, f.size_bytes, f.page_count, f.thumbnail_sha256 IS NOT NULL, pf.uploaded_at
         FROM part_files pf JOIN files f ON f.id = pf.file_id WHERE pf.part_id = ?1 ORDER BY pf.version DESC",
    )
    .bind(part_id)
    .fetch_all(&app.db.read)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|(version, sha256, mime, size_bytes, page_count, has_thumbnail, uploaded_at)| FileVersion {
                version,
                sha256,
                mime,
                size_bytes,
                page_count,
                has_thumbnail,
                uploaded_at,
            })
            .collect(),
    ))
}

#[cfg(test)]
mod tests {
    use super::fts_terms;

    #[test]
    fn fts_terms_quote_words_and_drop_operators() {
        assert_eq!(fts_terms("Fučík  march"), Some(r#"["\"Fučík\"*","\"march\"*"]"#.into()));
        assert_eq!(fts_terms("\" OR NEAR( or"), Some(r#"["\"OR\"*","\"NEAR\"*","\"or\"*"]"#.into()));
        assert_eq!(fts_terms("march march"), Some(r#"["\"march\"*"]"#.into()));
        assert_eq!(fts_terms("  ()  "), None);
    }
}
