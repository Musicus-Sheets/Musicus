//! Serves sheet files, thumbnails and system layouts by hash, after the access check.
//!
//! Plain requests are answered from the RAM cache (one shared copy for
//! every musician opening the same piece); `Range` requests stream from
//! disk via tower-http so a client can start with the first pages.

use crate::layout::apply_correction;
use crate::{
    access::librarian_flag,
    auth::AuthUser,
    blobs::is_sha256_hex,
    config::ClientCache,
    error::{ApiResult, AppError},
    part_visible,
    state::{App, AppState},
};
use axum::{
    body::{Body, Bytes},
    extract::{Path, Request, State},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use musicus_api::ErrorCode;
use musicus_api::layout::{FileLayout, LayoutState, PageCorrection, PageLayout};
use serde::Deserialize;
use tower::ServiceExt;
use tower_http::services::ServeFile;

/// `(mime, thumbnail sha)` of a file the user may read.
async fn readable_file(app: &App, user: &AuthUser, sha256: &str) -> ApiResult<(String, Option<String>)> {
    if !is_sha256_hex(sha256) {
        return Err(AppError::NotFound);
    }
    sqlx::query_as(concat!(
        "SELECT f.mime, f.thumbnail_sha256 FROM files f WHERE f.sha256 = ?3
           AND EXISTS (SELECT 1 FROM part_files pf JOIN parts p ON p.id = pf.part_id WHERE pf.file_id = f.id AND ",
        part_visible!(),
        ")"
    ))
    .bind(user.id)
    .bind(librarian_flag(user))
    .bind(sha256)
    .fetch_optional(&app.db.read)
    .await?
    .ok_or(AppError::NotFound)
}

pub async fn get_file(
    State(app): State<AppState>,
    user: AuthUser,
    Path(sha256): Path<String>,
    req: Request,
) -> ApiResult<Response> {
    let (mime, _) = readable_file(&app, &user, &sha256).await?;
    serve_blob(&app, &sha256, &mime, req).await
}

pub async fn get_thumbnail(
    State(app): State<AppState>,
    user: AuthUser,
    Path(sha256): Path<String>,
    req: Request,
) -> ApiResult<Response> {
    let (_, thumbnail) = readable_file(&app, &user, &sha256).await?;
    let thumbnail = thumbnail.ok_or(AppError::NotFound)?;
    serve_blob(&app, &thumbnail, "image/png", req).await
}

/// The music systems of every page (`GET /api/files/{sha256}/layout`), for
/// the system view of the reader. Same access rule as the file itself.
/// `pending` until the background worker has analysed the file.
pub async fn get_layout(
    State(app): State<AppState>,
    user: AuthUser,
    Path(sha256): Path<String>,
) -> ApiResult<axum::Json<FileLayout>> {
    readable_file(&app, &user, &sha256).await?;
    Ok(axum::Json(effective_layout(&app, &sha256).await?))
}

/// The detected layout with the librarians' corrections applied.
///
/// Stored values of a newer release are handled here, where they are read
/// (R-OPS-041, a-stored-fallbacks): an unknown state or a page list this
/// release cannot read ⇒ `failed` (whole pages, as if not analysed); a page
/// or a correction it cannot parse (e.g. a band kind it does not know) ⇒ that
/// page whole, listed in `unreadable_pages`. Nothing is written back, and the
/// worker does not redo a layout of a newer algorithm.
async fn effective_layout(app: &App, sha256: &str) -> ApiResult<FileLayout> {
    let row: Option<(i64, i64, String, String)> = sqlx::query_as(
        "SELECT f.id, l.algorithm, l.state, l.pages FROM file_layouts l JOIN files f ON f.id = l.file_id
         WHERE f.sha256 = ?1",
    )
    .bind(sha256)
    .fetch_optional(&app.db.read)
    .await?;
    let sha256 = sha256.to_string();
    let Some((file_id, algorithm, state, pages)) = row else {
        return Ok(FileLayout {
            sha256,
            state: LayoutState::Pending,
            algorithm: 0,
            pages: vec![],
            unreadable_pages: vec![],
        });
    };
    let failed = || FileLayout {
        sha256: sha256.clone(),
        state: LayoutState::Failed,
        algorithm,
        pages: vec![],
        unreadable_pages: vec![],
    };
    if state != "done" {
        return Ok(failed());
    }
    let Some((mut pages, mut unreadable)) = read_pages(&pages) else {
        tracing::warn!("system layout of {sha256} not readable by this release");
        return Ok(failed());
    };
    let corrections: Vec<(i64, String)> =
        sqlx::query_as("SELECT page, data FROM layout_corrections WHERE file_id = ?1 ORDER BY page")
            .bind(file_id)
            .fetch_all(&app.db.read)
            .await?;
    for (page, data) in corrections {
        let Some(index) = u32::try_from(page).ok() else { continue };
        let Some(p) = pages.get_mut(index as usize) else { continue };
        if unreadable.contains(&index) {
            // The detection is not readable: shown whole, the correction only marked.
            p.corrected = true;
            continue;
        }
        *p = match serde_json::from_str::<PageCorrection>(&data) {
            Ok(c) if c.validate().is_ok() => apply_correction(p, &c),
            _ => {
                unreadable.push(index);
                whole_page(p.width_mm, p.height_mm, true)
            }
        };
    }
    unreadable.sort_unstable();
    Ok(FileLayout { sha256, state: LayoutState::Done, algorithm, pages, unreadable_pages: unreadable })
}

/// A page shown whole because its stored layout is not readable.
fn whole_page(width_mm: f32, height_mm: f32, corrected: bool) -> PageLayout {
    PageLayout { width_mm, height_mm, confidence: 0.0, full_page: true, regions: vec![], corrected }
}

/// The stored pages of a detection, each parsed on its own: a page this
/// release cannot parse becomes a whole page (its size is read leniently) and
/// its index is returned too. `None` if not even that works.
fn read_pages(json: &str) -> Option<(Vec<PageLayout>, Vec<u32>)> {
    let values: Vec<serde_json::Value> = serde_json::from_str(json).ok()?;
    let (mut pages, mut unreadable) = (Vec::with_capacity(values.len()), Vec::new());
    for (index, v) in values.iter().enumerate() {
        match PageLayout::deserialize(v) {
            Ok(p) => pages.push(p),
            Err(e) => {
                tracing::warn!("page {index} of a system layout not readable by this release: {e}");
                let mm = |key: &str| v.get(key).and_then(serde_json::Value::as_f64).map(|x| x as f32);
                pages.push(whole_page(mm("width_mm")?, mm("height_mm")?, false));
                unreadable.push(u32::try_from(index).ok()?);
            }
        }
    }
    Some((pages, unreadable))
}

/// Id and page count of an analysed file, for corrections (librarians only).
async fn correctable(app: &App, user: &AuthUser, sha256: &str, page: i64) -> ApiResult<i64> {
    user.require_librarian()?;
    readable_file(app, user, sha256).await?;
    let (file_id, pages): (i64, String) = sqlx::query_as(
        "SELECT f.id, l.pages FROM files f JOIN file_layouts l ON l.file_id = f.id
         WHERE f.sha256 = ?1 AND l.state = 'done'",
    )
    .bind(sha256)
    .fetch_optional(&app.db.read)
    .await?
    .ok_or(AppError::Conflict(ErrorCode::SystemsNotDetected))?;
    let count = serde_json::from_str::<Vec<serde_json::Value>>(&pages).map_err(AppError::internal)?.len() as i64;
    if !(0..count).contains(&page) {
        return Err(AppError::NotFound);
    }
    Ok(file_id)
}

/// A librarian's correction of one page (`PUT /api/files/{sha256}/layout/pages/{page}`,
/// R-READ-016). Applies to every member; survives new detection runs.
pub async fn put_correction(
    State(app): State<AppState>,
    user: AuthUser,
    Path((sha256, page)): Path<(String, i64)>,
    axum::Json(c): axum::Json<PageCorrection>,
) -> ApiResult<axum::Json<FileLayout>> {
    let file_id = correctable(&app, &user, &sha256, page).await?;
    c.validate().map_err(AppError::BadRequest)?;
    sqlx::query(
        "INSERT INTO layout_corrections (file_id, page, data, updated_by, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (file_id, page) DO UPDATE SET data = ?3, updated_by = ?4, updated_at = ?5",
    )
    .bind(file_id)
    .bind(page)
    .bind(serde_json::to_string(&c).map_err(AppError::internal)?)
    .bind(user.id)
    .bind(crate::db::now())
    .execute(&app.db.write)
    .await?;
    Ok(axum::Json(effective_layout(&app, &sha256).await?))
}

/// Back to the automatic detection for one page.
pub async fn delete_correction(
    State(app): State<AppState>,
    user: AuthUser,
    Path((sha256, page)): Path<(String, i64)>,
) -> ApiResult<axum::Json<FileLayout>> {
    let file_id = correctable(&app, &user, &sha256, page).await?;
    sqlx::query("DELETE FROM layout_corrections WHERE file_id = ?1 AND page = ?2")
        .bind(file_id)
        .bind(page)
        .execute(&app.db.write)
        .await?;
    Ok(axum::Json(effective_layout(&app, &sha256).await?))
}

pub(crate) async fn serve_blob(app: &App, sha256: &str, mime: &str, req: Request) -> ApiResult<Response> {
    let etag = format!("\"{sha256}\"");
    let cache_control = match app.settings.client_cache {
        ClientCache::None => "private, no-store",
        ClientCache::Immutable => "private, max-age=31536000, immutable",
    };
    let not_modified = req
        .headers()
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(',').any(|t| t.trim() == etag || t.trim() == "*"));

    let mut response = if not_modified {
        StatusCode::NOT_MODIFIED.into_response()
    } else if req.headers().contains_key(header::RANGE) {
        let path = app.blobs.path(sha256);
        ServeFile::new(path).oneshot(req).await.map_err(AppError::internal)?.map(Body::new)
    } else {
        let bytes = match app.cache.get(sha256) {
            Some(b) => b,
            None => {
                let b = Bytes::from(tokio::fs::read(app.blobs.path(sha256)).await?);
                app.cache.insert(sha256, b.clone());
                b
            }
        };
        let mut r = Response::new(Body::from(bytes));
        r.headers_mut().insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
        r
    };
    let headers = response.headers_mut();
    if !not_modified {
        headers.insert(header::CONTENT_TYPE, HeaderValue::from_str(mime).map_err(AppError::internal)?);
    }
    headers.insert(header::ETAG, HeaderValue::from_str(&etag).map_err(AppError::internal)?);
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static(cache_control));
    headers.remove(header::LAST_MODIFIED);
    Ok(response)
}
