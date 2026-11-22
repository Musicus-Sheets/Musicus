//! SQLite access: one writer connection (SQLite allows one writer at a time,
//! so the pool of one *is* the write queue) and a small pool of readers,
//! which never block on the writer in WAL mode.

use crate::updates;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions, SqliteSynchronous};
use std::{path::Path, time::Duration};

#[derive(Clone)]
pub struct Db {
    pub read: SqlitePool,
    pub write: SqlitePool,
}

impl Db {
    /// Opens (or creates) the database file and brings it to this program's
    /// version (a-update-runner): refuses a release build with previews, reads
    /// the version marker (refusing a database outside the downgrade window
    /// without writing anything, R-OPS-015), then runs the planned parts.
    pub async fn open(path: &Path, readers: u32) -> Result<(Self, updates::Report), updates::UpdateError> {
        let program = updates::Program::builtin()?;
        program.check_build()?;
        let write = Self::writer(path).await?;
        let plan = updates::inspect(&write, program, now()).await?;
        // A snapshot of the database before the updates goes here (R-OPS-019, when `plan.has_work()`).
        let report = updates::apply(&write, program, &plan, now()).await?;
        let read = Self::readers(path, readers).await?;
        Ok((Self { read, write }, report))
    }

    fn options(path: &Path) -> SqliteConnectOptions {
        SqliteConnectOptions::new()
            .filename(path)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(5))
            .pragma("cache_size", "-16000")
    }

    /// The single writer connection (creates an empty file if there is none).
    pub async fn writer(path: &Path) -> Result<SqlitePool, sqlx::Error> {
        SqlitePoolOptions::new().max_connections(1).connect_with(Self::options(path).create_if_missing(true)).await
    }

    async fn readers(path: &Path, readers: u32) -> Result<SqlitePool, sqlx::Error> {
        SqlitePoolOptions::new().max_connections(readers.max(1)).connect_with(Self::options(path).read_only(true)).await
    }

    /// Writes a consistent copy of the database while the server may run.
    pub async fn backup_to(&self, target: &Path) -> Result<(), sqlx::Error> {
        sqlx::query("VACUUM INTO ?1").bind(target.to_string_lossy().into_owned()).execute(&self.write).await?;
        Ok(())
    }
}

/// Current time in Unix seconds.
pub fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// JSON array literal for `json_each(?)`, the way to pass id lists to SQLite.
pub fn json_ids(ids: &[i64]) -> String {
    serde_json::to_string(ids).expect("ids serialise")
}
