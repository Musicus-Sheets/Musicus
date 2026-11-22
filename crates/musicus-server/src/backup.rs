//! The commands `backup`, `restore` and `retry-update`, and the lock that
//! tells `restore` whether a server uses the data directory (R-OPS-021,
//! R-OPS-022, R-OPS-020; a-snapshots, a-data-lock, a-restart-guard).
//!
//! `backup` and the check of a file to restore open the database read-only
//! and never run the update runner, so they work with any release's data.

use crate::{
    db::Db,
    updates::{guard, snapshot},
};
use sqlx::SqlitePool;
use std::{
    fs::File,
    path::{Path, PathBuf},
};

pub type Result<T> = std::result::Result<T, String>;

/// Name of the database file in the data directory.
pub const DB_FILE: &str = "musicus.sqlite";
/// Name of the lock file in the data directory.
pub const LOCK_FILE: &str = "musicus.lock";

/// An operating-system lock on `<data>/musicus.lock`, held as long as the
/// value lives; the system releases it when the process ends, also after a
/// crash, so it is never stale. `serve` and `create-admin` hold it shared,
/// `restore` exclusively.
pub struct DataLock(#[allow(dead_code)] Option<File>);

impl DataLock {
    /// For commands that use the database (several may run at once).
    pub fn shared(data_dir: &Path) -> Result<Self> {
        Self::take(data_dir, true).map_err(|busy| match busy {
            Busy => format!("A restore is running on {}; start Musicus when it has finished.", data_dir.display()),
        })
    }

    /// For `restore`: nobody else may use the data directory.
    pub fn exclusive(data_dir: &Path) -> Result<Self> {
        Self::take(data_dir, false).map_err(|Busy| {
            format!(
                "A Musicus server (or another Musicus command) is using {}. Stop it first (with Docker: \
                 `docker compose stop musicus`), then run the restore again. Nothing was changed.",
                data_dir.display()
            )
        })
    }

    fn take(data_dir: &Path, shared: bool) -> std::result::Result<Self, Busy> {
        let open = || -> std::io::Result<File> {
            std::fs::create_dir_all(data_dir)?;
            File::options().read(true).write(true).create(true).truncate(false).open(data_dir.join(LOCK_FILE))
        };
        // A data directory where no lock file can be made (read-only) cannot be locked; go on without.
        let Ok(file) = open() else { return Ok(DataLock(None)) };
        let taken = if shared { fs4::FileExt::try_lock_shared(&file) } else { fs4::FileExt::try_lock(&file) };
        match taken {
            Ok(()) => Ok(DataLock(Some(file))),
            Err(fs4::TryLockError::WouldBlock) => Err(Busy),
            // File systems without locks: go on without (as before this lock existed).
            Err(fs4::TryLockError::Error(_)) => Ok(DataLock(None)),
        }
    }
}

struct Busy;

/// The release that last started on a database, from its version marker;
/// `None` if it has none or cannot be read.
async fn last_version(pool: &SqlitePool) -> Option<String> {
    sqlx::query_scalar("SELECT value FROM musicus_meta WHERE key = 'last_version'").fetch_optional(pool).await.ok()?
}

fn check_space(pool_needed: u64, dir: &Path) -> Result<()> {
    let free = snapshot::available(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    if free < pool_needed {
        return Err(format!(
            "Not enough free disk space in {}: about {} MB needed, {} MB free. Nothing was written.",
            dir.display(),
            pool_needed.div_ceil(1 << 20),
            free.div_ceil(1 << 20)
        ));
    }
    Ok(())
}

/// What `backup` wrote.
#[derive(Debug)]
pub struct Backup {
    /// The release that last used the copied database.
    pub version: Option<String>,
}

/// `backup --to <file>`: a consistent copy of the database as it is
/// (`VACUUM INTO` on a read-only connection; safe while the server runs),
/// whatever release wrote it (R-OPS-021).
pub async fn backup(data_dir: &Path, to: &Path) -> Result<Backup> {
    if to.exists() {
        return Err(format!("{} already exists", to.display()));
    }
    snapshot::sql_path(to).map_err(|e| e.to_string())?;
    let db = data_dir.join(DB_FILE);
    if !db.exists() {
        return Err(format!("There is no database at {}.", db.display()));
    }
    let pool = Db::read_only(&db).await.map_err(|e| format!("{}: {e}", db.display()))?;
    let result = async {
        let needed = snapshot::needed_bytes(&pool).await.map_err(|e| format!("{}: {e}", db.display()))?;
        let dir = to.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
        check_space(needed, dir)?;
        snapshot::copy_to(&pool, to).await.map_err(|e| format!("Writing {} failed: {e}", to.display()))?;
        Ok(Backup { version: last_version(&pool).await })
    }
    .await;
    pool.close().await;
    result
}

/// What `restore` did.
#[derive(Debug)]
pub struct Restore {
    /// The release that last used the restored database (`None`: a
    /// development database without marker).
    pub version: Option<String>,
    /// Where the replaced database was kept.
    pub replaced: Option<PathBuf>,
    /// Files the restored database refers to that are missing in `blobs/`.
    pub missing_files: u64,
    /// The restart guard note is still there (a-restart-guard).
    pub guard_note: Option<PathBuf>,
}

/// `restore --from <file>`: replaces the database of a stopped installation
/// with a backup or snapshot (R-OPS-022). Refuses, changing nothing, while
/// the data directory is in use or when `from` is not a Musicus database.
/// The replaced database is kept as `snapshots/replaced-<time>.sqlite`;
/// `blobs/` is not touched. The next start runs the update runner as usual.
pub async fn restore(data_dir: &Path, from: &Path, now: i64) -> Result<Restore> {
    let _lock = DataLock::exclusive(data_dir)?;
    let db = data_dir.join(DB_FILE);
    if !from.is_file() {
        return Err(format!("{} does not exist or is not a file. Nothing was changed.", from.display()));
    }
    if db.exists() && same_file(from, &db) {
        return Err(format!("{} is the database itself. Nothing was changed.", from.display()));
    }
    let not_musicus = |why: &str| format!("{} is not a Musicus database ({why}). Nothing was changed.", from.display());
    let source = Db::read_only(from).await.map_err(|e| not_musicus(&e.to_string()))?;
    let checked = async {
        let check: String = sqlx::query_scalar("PRAGMA quick_check")
            .fetch_one(&source)
            .await
            .map_err(|e| not_musicus(&e.to_string()))?;
        if check != "ok" {
            return Err(not_musicus(&format!("damaged: {check}")));
        }
        let tables: Vec<String> = sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type = 'table'")
            .fetch_all(&source)
            .await
            .map_err(|e| not_musicus(&e.to_string()))?;
        let has = |t: &str| tables.iter().any(|n| n == t);
        // The marker, or a development database of the sqlx time (recognised by the runner).
        if !(has("musicus_meta") || (has("_sqlx_migrations") && has("users"))) {
            return Err(not_musicus("no Musicus version marker"));
        }
        let needed = snapshot::needed_bytes(&source).await.map_err(|e| not_musicus(&e.to_string()))?;
        check_space(needed, data_dir)?;
        let staged = snapshot::staging_path(&db);
        let _ = std::fs::remove_file(&staged);
        snapshot::copy_to(&source, &staged).await.map_err(|e| format!("Copying {} failed: {e}", from.display()))?;
        Ok((last_version(&source).await, staged))
    }
    .await;
    source.close().await;
    let (version, staged) = checked?;

    let replaced = if db.exists() {
        let dir = snapshot::dir_of(&db);
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        Some(snapshot::unused(&dir, &format!("replaced-{}", snapshot::stamp(now))))
    } else {
        None
    };
    let done = match &replaced {
        Some(keep) => snapshot::replace(&db, &staged, keep),
        None => std::fs::rename(&staged, &db),
    };
    if let Err(e) = done {
        let _ = std::fs::remove_file(&staged);
        return Err(format!("Putting {} in place failed: {e}", from.display()));
    }
    let missing_files = missing_files(data_dir, &db).await;
    let guard_path = guard::path_of(&db);
    Ok(Restore { version, replaced, missing_files, guard_note: guard_path.exists().then_some(guard_path) })
}

fn same_file(a: &Path, b: &Path) -> bool {
    matches!((a.canonicalize(), b.canonicalize()), (Ok(a), Ok(b)) if a == b)
}

/// Counts files the database refers to that are not in `blobs/` (a backup
/// restored without the matching `blobs` folder).
async fn missing_files(data_dir: &Path, db: &Path) -> u64 {
    let Ok(pool) = Db::read_only(db).await else { return 0 };
    let hashes: Vec<String> = sqlx::query_scalar("SELECT sha256 FROM files").fetch_all(&pool).await.unwrap_or_default();
    pool.close().await;
    let blobs = crate::blobs::BlobStore::at(data_dir);
    hashes.iter().filter(|h| crate::blobs::is_sha256_hex(h) && !blobs.path(h).exists()).count() as u64
}

/// `retry-update`: removes the restart guard note, so the program whose
/// update failed tries again at its next start. True if there was one.
pub fn retry_update(data_dir: &Path) -> Result<bool> {
    let path = data_dir.join(guard::FILE);
    guard::clear(&path).map_err(|e| format!("{}: {e}", path.display()))
}
