//! Copies of the database around updates and restores (R-OPS-019,
//! R-OPS-020, R-OPS-022; a-snapshots): the snapshot before the updates of a
//! start, the free-space check for it, keeping the newest three, and putting
//! a copy in place of the database file (keeping the replaced one).
//!
//! Layout of `<data>/snapshots/`:
//!
//! * `<UTC time>-<from>-to-<to>.sqlite`: snapshot before the updates of a
//!   start, e.g. `20261122T143000Z-1.0.0-to-1.1.0.sqlite` (`from` is
//!   `unknown` for a database without a version marker). The newest three are
//!   kept; older ones are deleted only after a new one is complete.
//! * `failed-<UTC time>.sqlite`: the database as a failed update left it, for
//!   diagnosis (never deleted by the program).
//! * `replaced-<UTC time>.sqlite`: the database that `restore` replaced
//!   (never deleted by the program).
//!
//! Files (blobs) are never copied: they are immutable and only added
//! (R-FILE-004), so every copy here stays consistent with `blobs/`. A future
//! clean-up of unused files must count files referenced by any database in
//! this folder as used (R-FILE-009).

use sqlx::SqlitePool;
use std::{
    io,
    path::{Path, PathBuf},
};

/// Snapshots kept before updates (R-OPS-019).
pub const KEEP: usize = 3;

/// Name of the folder below the data directory.
pub const DIR: &str = "snapshots";

/// The snapshot folder of the data directory that holds `db`.
pub fn dir_of(db: &Path) -> PathBuf {
    db.parent().unwrap_or(Path::new(".")).join(DIR)
}

/// `YYYYMMDDTHHMMSSZ` (UTC), sortable and valid in file names on every system.
pub fn stamp(unix: i64) -> String {
    let (days, secs) = (unix.div_euclid(86_400), unix.rem_euclid(86_400));
    let (y, m, d) = super::window::civil_from_days(days);
    format!("{y:04}{m:02}{d:02}T{:02}{:02}{:02}Z", secs / 3600, secs / 60 % 60, secs % 60)
}

/// A path for `name` + `.sqlite` in `dir` that does not exist yet (a counter
/// is appended if two copies are made in the same second).
pub fn unused(dir: &Path, name: &str) -> PathBuf {
    let mut path = dir.join(format!("{name}.sqlite"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{name}-{n}.sqlite"));
        n += 1;
    }
    path
}

/// A path SQLite can take as text (finding 7 of the portability list: a path
/// that is not valid UTF-8 would be changed on the way).
pub fn sql_path(path: &Path) -> io::Result<&str> {
    path.to_str().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, format!("{} is not a valid UTF-8 path", path.display()))
    })
}

/// Bytes a `VACUUM INTO` copy of the database needs: its used pages plus a
/// margin of a tenth (at least 8 MiB) for the updates themselves.
pub async fn needed_bytes(pool: &SqlitePool) -> sqlx::Result<u64> {
    let (pages, free, size): (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT page_count FROM pragma_page_count), (SELECT freelist_count FROM pragma_freelist_count),
                (SELECT page_size FROM pragma_page_size)",
    )
    .fetch_one(pool)
    .await?;
    let used = ((pages - free).max(0) * size) as u64;
    Ok(used + (used / 10).max(8 << 20))
}

/// Free space for an unprivileged process in the file system holding `dir`.
pub fn available(dir: &Path) -> io::Result<u64> {
    fs4::available_space(dir)
}

/// Writes a consistent copy of the database behind `pool` to `target`
/// (`VACUUM INTO`, safe while others read and write), first as
/// `<target>.partial`, renamed when complete.
pub async fn copy_to(pool: &SqlitePool, target: &Path) -> Result<(), CopyError> {
    let partial = PathBuf::from(format!("{}.partial", target.display()));
    let _ = std::fs::remove_file(&partial);
    let result = sqlx::query("VACUUM INTO ?1").bind(sql_path(&partial)?.to_owned()).execute(pool).await;
    if let Err(e) = result {
        let _ = std::fs::remove_file(&partial);
        return Err(e.into());
    }
    std::fs::File::open(&partial)?.sync_all()?;
    std::fs::rename(&partial, target)?;
    sync_dir(target.parent().unwrap_or(Path::new(".")));
    Ok(())
}

/// Snapshots before updates in `dir`, oldest first.
pub fn list(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Ok(vec![]) };
    let mut found = Vec::new();
    for entry in entries {
        let path = entry?.path();
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let b = name.as_bytes();
        let stamped = b.len() > 16 && b[..8].iter().all(u8::is_ascii_digit) && b[8] == b'T' && b[15] == b'Z';
        if stamped && name.ends_with(".sqlite") {
            let modified = path.metadata()?.modified()?;
            found.push((name[..16].to_owned(), modified, path));
        }
    }
    found.sort();
    Ok(found.into_iter().map(|(_, _, p)| p).collect())
}

/// Deletes all but the newest [`KEEP`] snapshots, and leftovers of
/// interrupted copies.
pub fn prune(dir: &Path) -> io::Result<Vec<PathBuf>> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().is_some_and(|e| e == "partial") {
            std::fs::remove_file(&path)?;
        }
    }
    let all = list(dir)?;
    let old = all[..all.len().saturating_sub(KEEP)].to_vec();
    for path in &old {
        std::fs::remove_file(path)?;
    }
    Ok(old)
}

/// Moves the database file `db` (with its `-wal`; the `-shm` index is
/// deleted) to `keep_as` and puts the complete copy `ready` in its place.
/// `ready` must be in the same folder as `db`, so the final step is one
/// rename. Nobody may have the database open.
pub fn replace(db: &Path, ready: &Path, keep_as: &Path) -> io::Result<()> {
    set_aside(db, keep_as)?;
    std::fs::rename(ready, db)?;
    sync_dir(db.parent().unwrap_or(Path::new(".")));
    Ok(())
}

/// Moves the database file `db` with its `-wal` to `keep_as` and deletes the
/// `-shm` index, leaving no database at `db`.
pub fn set_aside(db: &Path, keep_as: &Path) -> io::Result<()> {
    let side = |p: &Path, s: &str| PathBuf::from(format!("{}{s}", p.display()));
    if db.exists() {
        std::fs::rename(db, keep_as)?;
        if side(db, "-wal").exists() {
            std::fs::rename(side(db, "-wal"), side(keep_as, "-wal"))?;
        }
    }
    if side(db, "-shm").exists() {
        std::fs::remove_file(side(db, "-shm"))?;
    }
    Ok(())
}

/// Copies the file `from` next to `db` as `<db>.restoring`, ready for [`replace`].
pub fn stage_copy(from: &Path, db: &Path) -> io::Result<PathBuf> {
    let staged = staging_path(db);
    std::fs::copy(from, &staged)?;
    std::fs::File::open(&staged)?.sync_all()?;
    Ok(staged)
}

/// Where a copy is prepared before it replaces `db`.
pub fn staging_path(db: &Path) -> PathBuf {
    PathBuf::from(format!("{}.restoring", db.display()))
}

/// Best effort: makes renames in `dir` durable (Unix only, like `blobs.rs`).
fn sync_dir(dir: &Path) {
    #[cfg(unix)]
    if let Ok(d) = std::fs::File::open(dir) {
        let _ = d.sync_all();
    }
    #[cfg(not(unix))]
    let _ = dir;
}

/// A copy that failed.
#[derive(Debug)]
pub enum CopyError {
    Io(io::Error),
    Db(sqlx::Error),
}

impl std::fmt::Display for CopyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CopyError::Io(e) => e.fmt(f),
            CopyError::Db(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for CopyError {}

impl From<io::Error> for CopyError {
    fn from(e: io::Error) -> Self {
        CopyError::Io(e)
    }
}

impl From<sqlx::Error> for CopyError {
    fn from(e: sqlx::Error) -> Self {
        CopyError::Db(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamps_sort_like_time() {
        assert_eq!(stamp(0), "19700101T000000Z");
        assert_eq!(stamp(1_795_350_896), "20261122T123456Z");
    }

    #[test]
    fn keeps_the_newest_three_and_ignores_other_files() {
        let dir = tempfile::tempdir().unwrap();
        let names = [
            "20260101T000000Z-1.0.0-to-1.1.0.sqlite",
            "20260301T000000Z-1.1.0-to-1.2.0.sqlite",
            "20260201T000000Z-1.0.0-to-1.1.0.sqlite",
            "20260401T000000Z-1.2.0-to-1.3.0.sqlite",
            "failed-20260101T000000Z.sqlite",
            "replaced-20250101T000000Z.sqlite",
            "20260501T000000Z-1.3.0-to-1.4.0.sqlite.partial",
            "notes.txt",
        ];
        for n in names {
            std::fs::write(dir.path().join(n), b"x").unwrap();
        }
        let gone = prune(dir.path()).unwrap();
        assert_eq!(gone, [dir.path().join(names[0])]);
        let mut left: Vec<String> =
            std::fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into()).collect();
        left.sort();
        assert_eq!(
            left,
            [
                "20260201T000000Z-1.0.0-to-1.1.0.sqlite",
                "20260301T000000Z-1.1.0-to-1.2.0.sqlite",
                "20260401T000000Z-1.2.0-to-1.3.0.sqlite",
                "failed-20260101T000000Z.sqlite",
                "notes.txt",
                "replaced-20250101T000000Z.sqlite",
            ]
        );
    }
}
