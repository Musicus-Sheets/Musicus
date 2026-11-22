//! One start on a database file: the guarded sequence around the runner
//! (`docs/technical/chapters/14-updates.typ`, start-up sequence; a-snapshots,
//! a-restart-guard):
//!
//! 1. refuse a release build with previews (R-OPS-024);
//! 2. refuse at once if the same program failed here before ([`guard`]);
//! 3. [`inspect`] (writes nothing; refuses outside the window, R-OPS-015);
//! 4. if there is work on existing data: check free space, write a snapshot,
//!    keep the newest three (R-OPS-019);
//! 5. [`apply`]; on failure put the snapshot back, keep the failed state,
//!    write the guard note and stop with a message (R-OPS-020).

use super::{
    Program, UpdateError, Version, apply, guard, inspect,
    runner::{Plan, Report},
    snapshot,
};
use sqlx::SqlitePool;
use std::{
    io,
    path::{Path, PathBuf},
};

/// What a start may vary (tests set the time and the free space).
#[derive(Clone, Copy)]
pub struct Options {
    /// Unix seconds.
    pub now: i64,
    /// Free bytes in the file system of a folder.
    pub available: fn(&Path) -> io::Result<u64>,
}

impl Default for Options {
    fn default() -> Self {
        Options { now: crate::db::now(), available: snapshot::available }
    }
}

/// Runs the start on the database file `db` and returns its writer pool.
pub async fn run(db: &Path, program: &Program, opts: &Options) -> Result<(SqlitePool, Report), UpdateError> {
    program.check_build()?;
    let guard_path = guard::path_of(db);
    let note = guard::read(&guard_path).map_err(|e| UpdateError::Io(format!("{}: {e}", guard_path.display())))?;
    let own = program.version;
    let fingerprint = program.fingerprint();
    if let Some(note) = &note
        && note.version == own.to_string()
        && note.fingerprint == fingerprint
    {
        return Err(UpdateError::Blocked { note: Box::new(note.clone()), path: guard_path });
    }

    let pool = crate::db::Db::writer(db).await?;
    let plan = match inspect(&pool, program, opts.now).await {
        Ok(plan) => plan,
        Err(e) => {
            pool.close().await;
            return Err(e);
        }
    };
    let snapshot = if plan.has_work() && !plan.is_fresh() {
        match take_snapshot(&pool, db, program, &plan, opts).await {
            Ok(path) => Some(path),
            Err(e) => {
                pool.close().await;
                return Err(e);
            }
        }
    } else {
        None
    };

    match apply(&pool, program, &plan, opts.now).await {
        Ok(mut report) => {
            report.snapshot = snapshot;
            if let Some(note) = note {
                // A newer or fixed program got past the failure; an older one leaves the note.
                if Version::parse(&note.version).is_none_or(|v| v <= own) {
                    let _ = guard::clear(&guard_path);
                } else {
                    tracing::warn!(
                        "Musicus {} failed to update this data before (see {}); it stays blocked",
                        note.version,
                        guard_path.display()
                    );
                }
            }
            Ok((pool, report))
        }
        Err(cause) => {
            pool.close().await;
            Err(restore_after(db, program, &plan, cause, snapshot, opts.now))
        }
    }
}

async fn take_snapshot(
    pool: &SqlitePool,
    db: &Path,
    program: &Program,
    plan: &Plan,
    opts: &Options,
) -> Result<PathBuf, UpdateError> {
    let dir = snapshot::dir_of(db);
    let fail = |e: &dyn std::fmt::Display| UpdateError::Snapshot { dir: dir.clone(), error: e.to_string() };
    std::fs::create_dir_all(&dir).map_err(|e| fail(&e))?;
    let needed = snapshot::needed_bytes(pool).await?;
    let available = (opts.available)(&dir).map_err(|e| fail(&e))?;
    if available < needed {
        return Err(UpdateError::NoSpace { needed, available, dir });
    }
    let from = plan.last_version.as_deref().unwrap_or("unknown");
    let name = format!("{}-{from}-to-{}", snapshot::stamp(opts.now), program.version);
    let path = snapshot::unused(&dir, &name);
    snapshot::copy_to(pool, &path).await.map_err(|e| fail(&e))?;
    // Older snapshots go only once the new one is complete.
    match snapshot::prune(&dir) {
        Ok(gone) => {
            for p in gone {
                tracing::info!("deleted the old snapshot {}", p.display());
            }
        }
        Err(e) => tracing::warn!("could not delete old snapshots in {}: {e}", dir.display()),
    }
    tracing::info!("snapshot of the database before the updates: {}", path.display());
    Ok(path)
}

/// After a failed `apply` (pools closed): keeps the failed state, puts the
/// snapshot back, writes the guard note, and returns the message.
fn restore_after(
    db: &Path,
    program: &Program,
    plan: &Plan,
    cause: UpdateError,
    snapshot: Option<PathBuf>,
    now: i64,
) -> UpdateError {
    let data = db.parent().unwrap_or(Path::new("."));
    let rel = |p: &Path| p.strip_prefix(data).unwrap_or(p).display().to_string();
    let dir = snapshot::dir_of(db);
    let result = (|| -> io::Result<PathBuf> {
        std::fs::create_dir_all(&dir)?;
        let failed = snapshot::unused(&dir, &format!("failed-{}", snapshot::stamp(now)));
        match &snapshot {
            Some(s) => snapshot::replace(db, &snapshot::stage_copy(s, db)?, &failed)?,
            None => snapshot::set_aside(db, &failed)?,
        }
        Ok(failed)
    })();
    let failed = match result {
        Ok(failed) => failed,
        Err(e) => {
            return UpdateError::RestoreFailed { cause: Box::new(cause), snapshot, error: e.to_string() };
        }
    };
    let update = match &cause {
        UpdateError::Failed { update, part, .. } => format!("{update} ({part} part)"),
        _ => "recording the version".to_owned(),
    };
    let note = guard::Note {
        version: program.version.to_string(),
        fingerprint: program.fingerprint(),
        update,
        at: now,
        previous: plan.last_version.clone(),
        snapshot: snapshot.as_deref().map(rel),
        failed_copy: Some(rel(&failed)),
        error: cause.to_string(),
    };
    let guard_path = guard::path_of(db);
    if let Err(e) = guard::write(&guard_path, &note) {
        tracing::warn!("could not write {}: {e}", guard_path.display());
    }
    UpdateError::Restored { cause: Box::new(cause), note: Box::new(note), guard: guard_path }
}
