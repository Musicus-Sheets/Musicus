//! The restart guard after a failed update (R-OPS-020, a-restart-guard).
//!
//! When an update fails and the snapshot is put back, the start writes
//! `<data>/update-failed.json`. A later start of the *same program* (same
//! version and the same update texts, [`super::Program::fingerprint`]) refuses
//! at once, before it opens the database, so a restart policy (Docker
//! `restart: unless-stopped`) neither repeats the failing update nor writes
//! more snapshots. Every other program starts normally: an older release
//! (going back) leaves the note in place; a newer or fixed program of at
//! least the noted version removes it once it started successfully.
//! `musicus-server retry-update` (or deleting the file) allows the same
//! program to try again.

use serde::{Deserialize, Serialize};
use std::{
    io,
    path::{Path, PathBuf},
};

/// File name below the data directory.
pub const FILE: &str = "update-failed.json";

/// What the note records.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Note {
    /// The program whose update failed.
    pub version: String,
    pub fingerprint: String,
    /// The failing update and part (`0011-topic`, `additive`), or a description.
    pub update: String,
    /// Unix seconds.
    pub at: i64,
    /// The release that ran on the data before (marker `last_version`).
    pub previous: Option<String>,
    /// The snapshot that was put back (relative to the data directory).
    pub snapshot: Option<String>,
    /// Where the failed state was kept (relative to the data directory).
    pub failed_copy: Option<String>,
    /// The error text.
    pub error: String,
}

/// The note's path for the data directory holding `db`.
pub fn path_of(db: &Path) -> PathBuf {
    db.parent().unwrap_or(Path::new(".")).join(FILE)
}

/// The note, if there is one. A note that cannot be read counts as a note
/// of an unknown program (it blocks nobody but is reported).
pub fn read(path: &Path) -> io::Result<Option<Note>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes).ok()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

pub fn write(path: &Path, note: &Note) -> io::Result<()> {
    let text = serde_json::to_vec_pretty(note).map_err(io::Error::other)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

/// Removes the note; true if there was one.
pub fn clear(path: &Path) -> io::Result<bool> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}
