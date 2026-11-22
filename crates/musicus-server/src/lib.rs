//! Musicus server: a sheet music library with group-based access control,
//! content-addressed file storage and private annotations.
//!
//! One process holds everything: the REST API, the static web client, the
//! SQLite database (WAL mode, one writer, several readers) and the file
//! store on disk.

pub mod access;
pub mod auth;
pub mod blobs;
pub mod cache;
pub mod config;
pub mod db;
pub mod error;
pub mod layout;
pub mod part_index;
pub mod processors;
pub mod routes;
pub mod state;
pub mod updates;

use std::path::Path;

/// Opens the data directory: database (brought to this version by the update
/// runner, a-update-runner) and file store.
pub async fn open_state(
    data_dir: &Path,
    cache_bytes: u64,
    settings: state::Settings,
) -> anyhow_like::Result<state::AppState> {
    std::fs::create_dir_all(data_dir)?;
    let readers = std::thread::available_parallelism().map(|n| n.get() as u32).unwrap_or(2).clamp(2, 8);
    let (db, report) = db::Db::open(&data_dir.join("musicus.sqlite"), readers).await?;
    if let Some(from) = &report.upgraded_from {
        tracing::info!("database last used by Musicus {from}; now {}", env!("CARGO_PKG_VERSION"));
    }
    for step in &report.steps {
        tracing::info!("database update: {step}");
    }
    if !report.ignored.is_empty() {
        tracing::info!("database updates of a newer program ignored: {}", report.ignored.join(", "));
    }
    // Derived data is rebuilt after another program used the database (d-update-model).
    if part_index::rebuild_if_needed(&db, report.other_ran_last).await? {
        tracing::info!("rebuilt the search index of part names");
    }
    let blobs = blobs::BlobStore::open(data_dir)?;
    Ok(state::App::new(db, blobs, cache_bytes, settings))
}

/// When the database has no users yet, creates `admin` and returns a
/// one-time invite token for it.
pub async fn bootstrap_admin(app: &state::App) -> anyhow_like::Result<Option<String>> {
    let users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users").fetch_one(&app.db.read).await?;
    if users > 0 {
        return Ok(None);
    }
    let id = routes::users::insert_user(app, "admin", "Administrator", musicus_api::Role::Admin).await?;
    Ok(Some(routes::users::issue_invite(app, id).await?.token))
}

/// Minimal boxed error for setup code outside request handling.
pub mod anyhow_like {
    pub type Error = Box<dyn std::error::Error + Send + Sync>;
    pub type Result<T> = std::result::Result<T, Error>;
}
