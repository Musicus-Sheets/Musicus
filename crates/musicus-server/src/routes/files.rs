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
use musicus_api::layout::{FileLayout, LayoutState, PageCorrection, PageLayout};
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
        return Ok(FileLayout { sha256, state: LayoutState::Pending, algorithm: 0, pages: vec![] });
    };
    let mut pages: Vec<PageLayout> = serde_json::from_str(&pages).map_err(AppError::internal)?;
    let corrections: Vec<(i64, String)> =
        sqlx::query_as("SELECT page, data FROM layout_corrections WHERE file_id = ?1 ORDER BY page")
            .bind(file_id)
            .fetch_all(&app.db.read)
            .await?;
    for (page, data) in corrections {
        if let Some(p) = usize::try_from(page).ok().and_then(|i| pages.get_mut(i)) {
            let c: PageCorrection = serde_json::from_str(&data).map_err(AppError::internal)?;
            *p = apply_correction(p, &c);
        }
    }
    let state = if state == "done" { LayoutState::Done } else { LayoutState::Failed };
    Ok(FileLayout { sha256, state, algorithm, pages })
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
    .ok_or_else(|| AppError::Conflict("the systems of this file have not been detected yet".into()))?;
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
    c.validate().map_err(|e| AppError::BadRequest(e.into()))?;
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

async fn serve_blob(app: &App, sha256: &str, mime: &str, req: Request) -> ApiResult<Response> {
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
