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
    /// version (a-update-runner, [`updates::start::run`]): refuses a release
    /// build with previews or a start that failed before (restart guard),
    /// reads the version marker (refusing a database outside the downgrade
    /// window without writing anything, R-OPS-015), writes a snapshot if there
    /// is work (R-OPS-019), then runs the planned parts and puts the snapshot
    /// back if one fails (R-OPS-020).
    pub async fn open(path: &Path, readers: u32) -> Result<(Self, updates::Report), updates::UpdateError> {
        Self::open_with(path, readers, updates::Program::builtin()?, &updates::start::Options::default()).await
    }

    /// [`Db::open`] with another set of updates, time or free-space source (tests).
    pub async fn open_with(
        path: &Path,
        readers: u32,
        program: &updates::Program,
        opts: &updates::start::Options,
    ) -> Result<(Self, updates::Report), updates::UpdateError> {
        let (write, report) = updates::start::run(path, program, opts).await?;
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

    /// One read-only connection that changes nothing in the file: no journal
    /// mode, no updates, no new file (`backup`, `restore`; R-OPS-021).
    pub async fn read_only(path: &Path) -> Result<SqlitePool, sqlx::Error> {
        let opts = SqliteConnectOptions::new().filename(path).read_only(true).busy_timeout(Duration::from_secs(5));
        SqlitePoolOptions::new().max_connections(1).connect_with(opts).await
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
