//! Background analysis of files: one file at a time on one blocking thread,
//! newest file first, so a fresh upload is ready within seconds while older
//! files (backfill after an update, or after [`super::ALGORITHM_VERSION`]
//! was raised) are processed when nothing new is waiting. Requests never wait
//! for it; until a file is done its layout is `pending` and the reader shows
//! whole pages.

use super::{ALGORITHM_VERSION, analyse_pdf};
use crate::{db::now, processors::PDF_MIME, state::AppState};
use musicus_api::Id;
use std::time::Instant;

/// The next file without a current layout (newest first).
async fn next_file(app: &AppState) -> Result<Option<(Id, String)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT f.id, f.sha256 FROM files f LEFT JOIN file_layouts l ON l.file_id = f.id
         WHERE f.mime = ?1 AND (l.file_id IS NULL OR l.algorithm < ?2)
         ORDER BY f.id DESC LIMIT 1",
    )
    .bind(PDF_MIME)
    .bind(ALGORITHM_VERSION)
    .fetch_optional(&app.db.read)
    .await
}

/// Analyses one file if one is waiting. Returns `false` when there was none.
pub async fn run_once(app: &AppState) -> Result<bool, sqlx::Error> {
    let Some((file_id, sha256)) = next_file(app).await? else { return Ok(false) };
    let started = Instant::now();
    let result = match tokio::fs::read(app.blobs.path(&sha256)).await {
        // A panic in the renderer (JoinError) counts as a failed analysis.
        Ok(data) => tokio::task::spawn_blocking(move || analyse_pdf(data))
            .await
            .unwrap_or_else(|_| Err("the analysis panicked".into())),
        Err(e) => Err(e.to_string()),
    };
    let (state, pages) = match &result {
        Ok(pages) => {
            let systems: usize = pages
                .iter()
                .map(|p| p.regions.iter().filter(|r| r.kind == musicus_api::layout::RegionKind::System).count())
                .sum();
            tracing::info!(
                "system layout of {sha256}: {} pages, {systems} systems, {} ms",
                pages.len(),
                started.elapsed().as_millis()
            );
            ("done", serde_json::to_string(pages).unwrap_or_else(|_| "[]".into()))
        }
        Err(e) => {
            tracing::warn!("system layout of {sha256} failed: {e}");
            ("failed", "[]".into())
        }
    };
    sqlx::query(
        "INSERT INTO file_layouts (file_id, algorithm, state, pages, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (file_id) DO UPDATE SET algorithm = ?2, state = ?3, pages = ?4, updated_at = ?5",
    )
    .bind(file_id)
    .bind(ALGORITHM_VERSION)
    .bind(state)
    .bind(pages)
    .bind(now())
    .execute(&app.db.write)
    .await?;
    Ok(true)
}

/// Starts the worker: it works through all waiting files, then sleeps until
/// an upload calls `app.layout_jobs.notify_one()`.
pub fn spawn(app: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            match run_once(&app).await {
                Ok(true) => continue,
                Ok(false) => app.layout_jobs.notified().await,
                Err(e) => {
                    tracing::warn!("system layout worker: {e}");
                    tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                }
            }
        }
    })
}
