//! Snapshots before updates, the automatic restore after a failed update,
//! the restart guard, `backup` without the runner and `restore`
//! (R-OPS-019, R-OPS-020, R-OPS-021, R-OPS-022; a-snapshots,
//! a-restart-guard, a-data-lock), on temporary data directories. Each
//! "program" is an `updates::Program` with its own version and update files,
//! as different binaries would be.

use musicus_server::{
    backup::{self, DataLock},
    db::Db,
    updates::{Program, UpdateError, guard, start::Options, window::days_from_civil},
};
use sqlx::SqlitePool;
use std::path::{Path, PathBuf};

const BASE: &str = "-- @additive\nCREATE TABLE t (id INTEGER PRIMARY KEY, a INTEGER);\n";
const ADD_B: &str = "-- @additive\nALTER TABLE t ADD COLUMN b INTEGER;\nUPDATE t SET b = 2 * a;\n";
/// Fails after a change, so only the snapshot can undo the updates before it.
const BROKEN: &str = "-- @additive\nCREATE TABLE c (x);\nINSERT INTO missing_table VALUES (1);\n";
const FIXED: &str = "-- @additive\nCREATE TABLE c (x);\n";
const LINES: &str = "1.0\n1.1\n1.2\n1.3\n1.4\n";

fn program(version: &str, files: &[(&str, &'static str)]) -> Program {
    Program::new(version, LINES, files.iter().map(|(p, t)| (p.to_string(), *t)).collect(), false).unwrap()
}

fn day(d: u32) -> i64 {
    days_from_civil(2027, 3, d) * 86_400 + 12 * 3600
}

fn opts(now: i64) -> Options {
    Options { now, ..Options::default() }
}

fn db_path(dir: &Path) -> PathBuf {
    dir.join("musicus.sqlite")
}

/// One start of `program` on the data directory; closes the database again.
async fn start(dir: &Path, program: &Program, now: i64) -> Result<musicus_server::updates::Report, UpdateError> {
    let (db, report) = Db::open_with(&db_path(dir), 1, program, &opts(now)).await?;
    db.read.close().await;
    db.write.close().await;
    Ok(report)
}

fn snapshots(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir.join("snapshots")) else { return vec![] };
    // Only the databases (opening the failed copy here makes SQLite add `-wal`/`-shm` files).
    let mut names: Vec<String> = entries
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".sqlite"))
        .collect();
    names.sort();
    names
}

async fn columns(pool: &SqlitePool, table: &str) -> Vec<String> {
    sqlx::query_scalar("SELECT name FROM pragma_table_info(?1) ORDER BY cid").bind(table).fetch_all(pool).await.unwrap()
}

async fn last_version(pool: &SqlitePool) -> String {
    sqlx::query_scalar("SELECT value FROM musicus_meta WHERE key = 'last_version'").fetch_one(pool).await.unwrap()
}

/// A data directory last used by Musicus 1.0.0, with two rows in `t`.
async fn installed_1_0(dir: &Path) {
    start(dir, &program("1.0.0", &[("1.0.0.sql", BASE)]), day(1)).await.unwrap();
    let pool = Db::writer(&db_path(dir)).await.unwrap();
    sqlx::query("INSERT INTO t (a) VALUES (1), (2)").execute(&pool).await.unwrap();
    pool.close().await;
}

#[tokio::test]
async fn a_snapshot_is_written_only_when_there_is_work_and_three_are_kept() {
    let dir = tempfile::tempdir().unwrap();
    installed_1_0(dir.path()).await;
    assert!(snapshots(dir.path()).is_empty(), "a fresh install has nothing to keep");
    let p10 = program("1.0.0", &[("1.0.0.sql", BASE)]);
    let report = start(dir.path(), &p10, day(2)).await.unwrap();
    assert!(report.snapshot.is_none() && snapshots(dir.path()).is_empty(), "no pending updates, no snapshot");

    let p11 = program("1.1.0", &[("1.0.0.sql", BASE), ("1.1.0.sql", ADD_B)]);
    let report = start(dir.path(), &p11, day(3)).await.unwrap();
    assert_eq!(snapshots(dir.path()), ["20270303T120000Z-1.0.0-to-1.1.0.sqlite"]);
    assert_eq!(report.snapshot.unwrap(), dir.path().join("snapshots/20270303T120000Z-1.0.0-to-1.1.0.sqlite"));
    // The snapshot is a valid database of the state before.
    let snap = Db::read_only(&dir.path().join("snapshots").join(&snapshots(dir.path())[0])).await.unwrap();
    assert_eq!(columns(&snap, "t").await, ["id", "a"]);
    assert_eq!(last_version(&snap).await, "1.0.0");
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM t").fetch_one(&snap).await.unwrap();
    assert_eq!(rows, 2);
    snap.close().await;

    let a = "-- @additive\nSELECT 1;\n";
    let mut files = vec![("1.0.0.sql", BASE), ("1.1.0.sql", ADD_B)];
    for (n, (version, file)) in
        [("1.2.0", "1.2.0.sql"), ("1.3.0", "1.3.0.sql"), ("1.4.0", "1.4.0.sql")].into_iter().enumerate()
    {
        files.push((file, a));
        start(dir.path(), &program(version, &files), day(4 + n as u32)).await.unwrap();
    }
    assert_eq!(
        snapshots(dir.path()),
        [
            "20270304T120000Z-1.1.0-to-1.2.0.sqlite",
            "20270305T120000Z-1.2.0-to-1.3.0.sqlite",
            "20270306T120000Z-1.3.0-to-1.4.0.sqlite",
        ],
        "the fourth snapshot deleted the oldest"
    );
}

#[tokio::test]
async fn not_enough_space_refuses_before_any_update() {
    let dir = tempfile::tempdir().unwrap();
    installed_1_0(dir.path()).await;
    let p11 = program("1.1.0", &[("1.0.0.sql", BASE), ("1.1.0.sql", ADD_B)]);
    let none = Options { now: day(3), available: |_| Ok(1 << 20) };
    let Err(err) = Db::open_with(&db_path(dir.path()), 1, &p11, &none).await else { panic!("started") };
    let msg = err.to_string();
    assert!(matches!(err, UpdateError::NoSpace { .. }), "{msg}");
    assert!(
        msg.contains("needs about 9 MB") && msg.contains("only 1 MB are free") && msg.contains("No update ran"),
        "{msg}"
    );
    assert!(snapshots(dir.path()).is_empty());
    let pool = Db::read_only(&db_path(dir.path())).await.unwrap();
    assert_eq!(columns(&pool, "t").await, ["id", "a"]);
    assert_eq!(last_version(&pool).await, "1.0.0");
}

#[tokio::test]
async fn a_failed_update_puts_the_snapshot_back_and_is_not_retried() {
    let dir = tempfile::tempdir().unwrap();
    installed_1_0(dir.path()).await;
    // 1.1.0 succeeds, 1.2.0 fails: without the snapshot, 1.1.0 would stay applied.
    let broken = program("1.2.0", &[("1.0.0.sql", BASE), ("1.1.0.sql", ADD_B), ("1.2.0.sql", BROKEN)]);
    let err = start(dir.path(), &broken, day(3)).await.unwrap_err();
    let msg = err.to_string();
    assert!(matches!(err, UpdateError::Restored { .. }), "{msg}");
    let snap = "20270303T120000Z-1.0.0-to-1.2.0.sqlite";
    for part in [
        "The database update 1.2.0 failed in its additive part",
        &format!("put back from the snapshot snapshots/{snap}"),
        "the data is unchanged",
        "snapshots/failed-20270303T120000Z.sqlite",
        "Musicus 1.0.0, again",
        "/issues",
        "retry-update",
    ] {
        assert!(msg.contains(part), "`{part}` missing in: {msg}");
    }
    assert_eq!(snapshots(dir.path()), [snap.to_owned(), "failed-20270303T120000Z.sqlite".to_owned()]);
    let pool = Db::read_only(&db_path(dir.path())).await.unwrap();
    assert_eq!(columns(&pool, "t").await, ["id", "a"], "the database equals the snapshot");
    assert_eq!(last_version(&pool).await, "1.0.0");
    pool.close().await;
    let failed = Db::read_only(&dir.path().join("snapshots/failed-20270303T120000Z.sqlite")).await.unwrap();
    assert_eq!(columns(&failed, "t").await, ["id", "a", "b"], "the failed state is kept for diagnosis");
    failed.close().await;
    let note = guard::read(&dir.path().join(guard::FILE)).unwrap().unwrap();
    assert_eq!((note.version.as_str(), note.update.as_str()), ("1.2.0", "1.2.0 (additive part)"));
    assert_eq!(note.previous.as_deref(), Some("1.0.0"));

    // A restart (Docker restart policy) refuses at once: no new snapshot, nothing changed.
    let err = start(dir.path(), &broken, day(4)).await.unwrap_err();
    let msg = err.to_string();
    assert!(matches!(err, UpdateError::Blocked { .. }), "{msg}");
    assert!(msg.contains("Musicus 1.2.0 does not start") && msg.contains("2027-03-03 12:00 UTC"), "{msg}");
    assert!(msg.contains("Musicus 1.0.0, or a fixed release") && msg.contains("retry-update"), "{msg}");
    assert_eq!(snapshots(dir.path()).len(), 2, "{:?}", snapshots(dir.path()));

    // The previous release starts again and leaves the note for 1.2.0.
    start(dir.path(), &program("1.0.0", &[("1.0.0.sql", BASE)]), day(5)).await.unwrap();
    assert!(dir.path().join(guard::FILE).exists());
    assert!(matches!(start(dir.path(), &broken, day(6)).await, Err(UpdateError::Blocked { .. })));

    // `retry-update` lets the same program try again (and fail again, safely).
    assert!(backup::retry_update(dir.path()).unwrap());
    assert!(!backup::retry_update(dir.path()).unwrap());
    assert!(matches!(start(dir.path(), &broken, day(7)).await, Err(UpdateError::Restored { .. })));

    // A fixed program (same version, other update text) tries, succeeds and removes the note.
    let fixed = program("1.2.0", &[("1.0.0.sql", BASE), ("1.1.0.sql", ADD_B), ("1.2.0.sql", FIXED)]);
    start(dir.path(), &fixed, day(8)).await.unwrap();
    assert!(!dir.path().join(guard::FILE).exists());
    let pool = Db::read_only(&db_path(dir.path())).await.unwrap();
    assert_eq!(columns(&pool, "t").await, ["id", "a", "b"]);
    assert_eq!(last_version(&pool).await, "1.2.0");
}

#[tokio::test]
async fn a_failed_first_start_leaves_an_empty_data_directory() {
    let dir = tempfile::tempdir().unwrap();
    let broken = program("1.0.0", &[("1.0.0.sql", BROKEN)]);
    let err = start(dir.path(), &broken, day(1)).await.unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("There was no data yet"), "{msg}");
    assert!(!db_path(dir.path()).exists());
    assert_eq!(snapshots(dir.path()), ["failed-20270301T120000Z.sqlite"]);
    assert!(matches!(start(dir.path(), &broken, day(2)).await, Err(UpdateError::Blocked { .. })));
}

#[tokio::test]
async fn backup_copies_a_newer_release_database_without_updating_it() {
    let dir = tempfile::tempdir().unwrap();
    // Written by a "newer" release that this program cannot start on.
    let newer = program("7.0.0", &[("7.0.0.sql", BASE)]);
    start(dir.path(), &newer, day(1)).await.unwrap();
    let pool = Db::writer(&db_path(dir.path())).await.unwrap();
    sqlx::query("INSERT INTO musicus_meta (key, value) VALUES ('oldest_allowed', '7.0.0')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO t (a) VALUES (5)").execute(&pool).await.unwrap();
    pool.close().await;
    assert!(matches!(Db::open(&db_path(dir.path()), 1).await, Err(UpdateError::TooOld { .. })));
    let before = std::fs::read(db_path(dir.path())).unwrap();

    let target = dir.path().join("copy.sqlite");
    let done = backup::backup(dir.path(), &target).await.unwrap();
    assert_eq!(done.version.as_deref(), Some("7.0.0"));
    assert_eq!(std::fs::read(db_path(dir.path())).unwrap(), before, "the database file is unchanged");
    let copy = Db::read_only(&target).await.unwrap();
    assert_eq!(last_version(&copy).await, "7.0.0");
    let a: i64 = sqlx::query_scalar("SELECT a FROM t").fetch_one(&copy).await.unwrap();
    assert_eq!(a, 5);
    let users: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master WHERE name = 'users'").fetch_one(&copy).await.unwrap();
    assert_eq!(users, 0, "no update of this program ran");
    copy.close().await;
    assert!(backup::backup(dir.path(), &target).await.unwrap_err().contains("already exists"));
    assert!(
        backup::backup(&dir.path().join("none"), &dir.path().join("x.sqlite"))
            .await
            .unwrap_err()
            .contains("no database")
    );
}

#[tokio::test]
async fn backup_works_while_the_server_writes() {
    let dir = tempfile::tempdir().unwrap();
    installed_1_0(dir.path()).await;
    let p10 = program("1.0.0", &[("1.0.0.sql", BASE)]);
    let (db, _) = Db::open_with(&db_path(dir.path()), 1, &p10, &opts(day(2))).await.unwrap();
    sqlx::query("INSERT INTO t (a) VALUES (3)").execute(&db.write).await.unwrap();
    let target = dir.path().join("online.sqlite");
    backup::backup(dir.path(), &target).await.unwrap();
    let copy = Db::read_only(&target).await.unwrap();
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM t").fetch_one(&copy).await.unwrap();
    assert_eq!(rows, 3, "the copy includes what is still in the WAL");
}

#[tokio::test]
async fn restore_replaces_the_database_and_keeps_the_old_one() {
    let dir = tempfile::tempdir().unwrap();
    installed_1_0(dir.path()).await;
    let backup_file = dir.path().join("b.sqlite");
    backup::backup(dir.path(), &backup_file).await.unwrap();
    // Later: upgraded to 1.1, more data.
    let p11 = program("1.1.0", &[("1.0.0.sql", BASE), ("1.1.0.sql", ADD_B)]);
    start(dir.path(), &p11, day(3)).await.unwrap();
    std::fs::create_dir_all(dir.path().join("blobs/ab")).unwrap();
    std::fs::write(dir.path().join("blobs/ab/keep"), b"blob").unwrap();

    // Refused while a server uses the data directory.
    let lock = DataLock::shared(dir.path()).unwrap();
    let err = backup::restore(dir.path(), &backup_file, day(4)).await.unwrap_err();
    assert!(err.contains("is using") && err.contains("Nothing was changed"), "{err}");
    drop(lock);

    // Refused: not a Musicus database (no SQLite at all, or foreign tables).
    let junk = dir.path().join("junk.sqlite");
    std::fs::write(&junk, vec![7u8; 8192]).unwrap();
    let err = backup::restore(dir.path(), &junk, day(4)).await.unwrap_err();
    assert!(err.contains("is not a Musicus database"), "{err}");
    let foreign = dir.path().join("foreign.sqlite");
    let pool = Db::writer(&foreign).await.unwrap();
    sqlx::query("CREATE TABLE other (x)").execute(&pool).await.unwrap();
    pool.close().await;
    let err = backup::restore(dir.path(), &foreign, day(4)).await.unwrap_err();
    assert!(err.contains("no Musicus version marker"), "{err}");
    let err = backup::restore(dir.path(), &db_path(dir.path()), day(4)).await.unwrap_err();
    assert!(err.contains("the database itself"), "{err}");
    assert!(
        !dir.path()
            .join("snapshots")
            .read_dir()
            .unwrap()
            .any(|e| { e.unwrap().file_name().to_string_lossy().starts_with("replaced-") }),
        "nothing was changed"
    );

    let done = backup::restore(dir.path(), &backup_file, day(4)).await.unwrap();
    assert_eq!(done.version.as_deref(), Some("1.0.0"));
    let kept = done.replaced.unwrap();
    assert_eq!(kept, dir.path().join("snapshots/replaced-20270304T120000Z.sqlite"));
    assert_eq!(std::fs::read(dir.path().join("blobs/ab/keep")).unwrap(), b"blob");
    let old = Db::read_only(&kept).await.unwrap();
    assert_eq!(last_version(&old).await, "1.1.0", "the replaced database is kept");
    old.close().await;
    let pool = Db::read_only(&db_path(dir.path())).await.unwrap();
    assert_eq!(last_version(&pool).await, "1.0.0");
    assert_eq!(columns(&pool, "t").await, ["id", "a"]);
    pool.close().await;

    // The next start upgrades the restored older backup as usual (with a snapshot).
    let report = start(dir.path(), &p11, day(5)).await.unwrap();
    assert_eq!(report.steps, ["1.1.0 additive part"]);
    assert!(report.snapshot.is_some());
}
