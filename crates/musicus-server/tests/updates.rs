//! The update runner on temporary databases (a-update-runner, d-update-model):
//! fresh install, upgrade, downgrade inside and outside the window, rebuild
//! after a downgrade, removals only when due, previews and their merge by
//! `scripts/release-migrations.py`, development databases of the
//! `sqlx::migrate!` time. Each "program" is an `updates::Program` with its own
//! version, release list and update files, as different binaries would be.

use musicus_server::{
    db::Db,
    updates::{self, Program, Report, UpdateError, window::days_from_civil},
};
use sqlx::SqlitePool;
use std::path::{Path, PathBuf};

const BASE: &str = "-- @additive\nCREATE TABLE t (id INTEGER PRIMARY KEY, a INTEGER);\n";
/// 1.1 replaces column `a` by `b` (= 2·a): additive copies, rebuild refills, removal drops `a`.
const U11: &str = "-- Column b replaces a.
-- @additive
ALTER TABLE t ADD COLUMN b INTEGER;
UPDATE t SET b = 2 * a;
-- @rebuild
UPDATE t SET b = 2 * a;
-- @removal
ALTER TABLE t DROP COLUMN a;
";
const LINES: &str = "1.0\n1.1\n1.2\n1.3\n1.4\n1.5\n";

fn program(version: &str, releases: &str, files: &[(&str, &'static str)]) -> Program {
    Program::new(version, releases, files.iter().map(|(p, t)| (p.to_string(), *t)).collect(), false).unwrap()
}

fn date(y: i64, m: u32, d: u32) -> i64 {
    days_from_civil(y, m, d) * 86_400 + 12 * 3600
}

async fn pool(dir: &Path) -> SqlitePool {
    Db::writer(&dir.join("musicus.sqlite")).await.unwrap()
}

/// One start of `program`: inspect, then apply.
async fn start(pool: &SqlitePool, program: &Program, now: i64) -> Result<Report, UpdateError> {
    program.check_build()?;
    let plan = updates::inspect(pool, program, now).await?;
    updates::apply(pool, program, &plan, now).await
}

async fn columns(pool: &SqlitePool, table: &str) -> Vec<String> {
    sqlx::query_scalar("SELECT name FROM pragma_table_info(?1) ORDER BY cid").bind(table).fetch_all(pool).await.unwrap()
}

async fn meta(pool: &SqlitePool, key: &str) -> Option<String> {
    sqlx::query_scalar("SELECT value FROM musicus_meta WHERE key = ?1").bind(key).fetch_optional(pool).await.unwrap()
}

async fn upgrades(pool: &SqlitePool) -> Vec<(String, String)> {
    sqlx::query_as("SELECT from_version, to_version FROM musicus_upgrades ORDER BY id").fetch_all(pool).await.unwrap()
}

#[tokio::test]
async fn fresh_install_runs_every_update_and_writes_the_marker() {
    let dir = tempfile::tempdir().unwrap();
    let db = pool(dir.path()).await;
    let p = program("1.1.0", "1.0\n", &[("1.0.0.sql", BASE), ("1.1.0.sql", U11)]);
    let report = start(&db, &p, date(2027, 1, 1)).await.unwrap();
    assert_eq!(report.steps, ["1.0.0 additive part", "1.1.0 additive part"]);
    assert!(!report.other_ran_last && report.upgraded_from.is_none());
    assert_eq!(columns(&db, "t").await, ["id", "a", "b"], "the removal of 1.1 waits: 1.0 may still start");
    assert_eq!(meta(&db, "marker_format").await.as_deref(), Some("1"));
    assert_eq!(meta(&db, "last_version").await.as_deref(), Some("1.1.0"));
    assert_eq!(meta(&db, "last_updates").await.as_deref(), Some(r#"["1.0.0","1.1.0"]"#));
    assert_eq!(meta(&db, "oldest_allowed").await, None);
    let rows: Vec<(String, String, String, Option<i64>)> =
        sqlx::query_as("SELECT name, kind, release, removal_at FROM musicus_updates ORDER BY name")
            .fetch_all(&db)
            .await
            .unwrap();
    assert_eq!(
        rows,
        [
            ("1.0.0".into(), "release".into(), "1.0.0".into(), None),
            ("1.1.0".into(), "release".into(), "1.1.0".into(), None)
        ]
    );
    assert!(upgrades(&db).await.is_empty());
    // A second start of the same program has nothing to do.
    let again = start(&db, &p, date(2027, 1, 2)).await.unwrap();
    assert!(again.steps.is_empty() && !again.other_ran_last);
}

/// Upgrade 1.0 → 1.1, downgrade to 1.0 (allowed, ignores 1.1's update), the
/// older release writes data, the upgrade again rebuilds 1.1's structures.
#[tokio::test]
async fn downgrade_inside_the_window_and_rebuild_after_it() {
    let dir = tempfile::tempdir().unwrap();
    let db = pool(dir.path()).await;
    let p10 = program("1.0.0", "1.0\n", &[("1.0.0.sql", BASE)]);
    let p11 = program("1.1.0", "1.0\n1.1\n", &[("1.0.0.sql", BASE), ("1.1.0.sql", U11)]);
    start(&db, &p10, date(2027, 1, 1)).await.unwrap();
    sqlx::query("INSERT INTO t (id, a) VALUES (1, 1)").execute(&db).await.unwrap();
    let up = start(&db, &p11, date(2027, 2, 1)).await.unwrap();
    assert_eq!(up.steps, ["1.1.0 additive part"]);
    assert_eq!(up.upgraded_from.as_deref(), Some("1.0.0"));
    // Downgrade: 1.0 knows nothing of 1.1.0 and ignores it.
    let down = start(&db, &p10, date(2027, 2, 2)).await.unwrap();
    assert_eq!(down.ignored, ["1.1.0"]);
    assert!(down.steps.is_empty() && down.other_ran_last);
    sqlx::query("INSERT INTO t (id, a) VALUES (2, 5)").execute(&db).await.unwrap();
    sqlx::query("UPDATE t SET a = 3 WHERE id = 1").execute(&db).await.unwrap();
    // Upgrade again: the rebuild refills b from what 1.0 left (R-OPS-016).
    let again = start(&db, &p11, date(2027, 2, 3)).await.unwrap();
    assert_eq!(again.steps, ["1.1.0 rebuild"]);
    let b: Vec<(i64, Option<i64>)> = sqlx::query_as("SELECT id, b FROM t ORDER BY id").fetch_all(&db).await.unwrap();
    assert_eq!(b, [(1, Some(6)), (2, Some(10))]);
    let rebuilt: Option<i64> =
        sqlx::query_scalar("SELECT rebuilt_at FROM musicus_updates WHERE name = '1.1.0'").fetch_one(&db).await.unwrap();
    assert_eq!(rebuilt, Some(date(2027, 2, 3)));
    // No rebuild when 1.1 itself ran last.
    assert!(start(&db, &p11, date(2027, 2, 4)).await.unwrap().steps.is_empty());
    assert_eq!(
        upgrades(&db).await,
        [("1.0.0".into(), "1.1.0".into()), ("1.1.0".into(), "1.0.0".into()), ("1.0.0".into(), "1.1.0".into())]
    );
}

/// The removal of 1.1 waits while 1.0 may start (rule b after the upgrade from
/// 1.0, restarted by a downgrade and re-upgrade), then runs and raises
/// `oldest_allowed`; 1.0 is then refused without any change (R-OPS-015).
#[tokio::test]
async fn removal_runs_only_when_due_and_older_releases_are_then_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("musicus.sqlite");
    let db = pool(dir.path()).await;
    let p10 = program("1.0.0", "1.0\n", &[("1.0.0.sql", BASE)]);
    let p11 = program("1.1.0", "1.0\n1.1\n", &[("1.0.0.sql", BASE), ("1.1.0.sql", U11)]);
    let p15 = program("1.5.0", LINES, &[("1.0.0.sql", BASE), ("1.1.0.sql", U11)]);
    start(&db, &p10, date(2027, 1, 1)).await.unwrap();
    start(&db, &p11, date(2027, 2, 1)).await.unwrap();
    // 1.5 on 1 March: 1.0.0 is a source until 1 May, 1.1.0 until 1 June.
    let r = start(&db, &p15, date(2027, 3, 1)).await.unwrap();
    assert!(r.steps.is_empty(), "{:?}", r.steps);
    assert_eq!(columns(&db, "t").await, ["id", "a", "b"]);
    // Back to 1.0 inside the window, then up to 1.5 on 2 June: 1.0.0 is a source again.
    start(&db, &p10, date(2027, 3, 2)).await.unwrap();
    let r = start(&db, &p15, date(2027, 6, 2)).await.unwrap();
    assert_eq!(r.steps, ["1.1.0 rebuild"], "an older release ran; the removal waits for the new source");
    assert_eq!(columns(&db, "t").await, ["id", "a", "b"]);
    // Three months later the window is 1.2–1.4 only: the removal of 1.1 is due.
    let r = start(&db, &p15, date(2027, 9, 3)).await.unwrap();
    assert_eq!(r.steps, ["1.1.0 removal part"]);
    assert_eq!(columns(&db, "t").await, ["id", "b"]);
    assert_eq!(meta(&db, "oldest_allowed").await.as_deref(), Some("1.1.0"));
    let removed: Option<i64> =
        sqlx::query_scalar("SELECT removal_at FROM musicus_updates WHERE name = '1.1.0'").fetch_one(&db).await.unwrap();
    assert_eq!(removed, Some(date(2027, 9, 3)));

    // 1.0 is refused and changes nothing (database byte-identical).
    db.close().await;
    let before = std::fs::read(&path).unwrap();
    let db = pool(dir.path()).await;
    let err = start(&db, &p10, date(2027, 9, 4)).await.unwrap_err();
    assert_eq!(
        err.to_string(),
        "This data was last used by Musicus 1.5.0. Musicus 1.0.0 can no longer use it; start Musicus 1.1.0 or newer."
    );
    assert_eq!(format!("{err:?}"), err.to_string(), "main prints it with {{:?}}");
    db.close().await;
    assert_eq!(std::fs::read(&path).unwrap(), before, "the refused start wrote nothing");
    // 1.1 (the release that brought the update) may still start.
    let db = pool(dir.path()).await;
    start(&db, &p11, date(2027, 9, 5)).await.unwrap();
    assert_eq!(meta(&db, "last_version").await.as_deref(), Some("1.1.0"));
}

/// Rule (c): patches of the same line replace each other at any time.
#[tokio::test]
async fn patches_of_the_same_line_replace_each_other() {
    let dir = tempfile::tempdir().unwrap();
    let db = pool(dir.path()).await;
    let files = [("1.0.0.sql", BASE), ("1.1.0.sql", U11)];
    let p150 = program("1.5.0", LINES, &files);
    let p157 = program("1.5.7", LINES, &files);
    start(&db, &p150, date(2027, 1, 1)).await.unwrap();
    // Removal due on a fresh 1.5 database: 1.1 was never a source, the window is 1.2–1.4.
    assert_eq!(columns(&db, "t").await, ["id", "b"]);
    for (i, p) in [&p157, &p150, &p157].into_iter().enumerate() {
        let r = start(&db, p, date(2028, 1, 1 + i as u32)).await.unwrap();
        assert!(r.steps.is_empty() && r.upgraded_from.is_some());
    }
    assert_eq!(upgrades(&db).await.len(), 3);
}

/// A newer program never runs removals while an unknown (newer) version ran
/// on the database, and an older program never runs any.
#[tokio::test]
async fn only_the_newest_program_runs_removals() {
    let dir = tempfile::tempdir().unwrap();
    let db = pool(dir.path()).await;
    let p16 = program("1.6.0", "1.0\n1.1\n1.2\n1.3\n1.4\n1.5\n1.6\n", &[("1.0.0.sql", BASE)]);
    let p15 = program("1.5.0", LINES, &[("1.0.0.sql", BASE), ("1.1.0.sql", U11)]);
    // 1.6 (which, in this test, does not have 1.1.0) creates the database; 1.5 starts later.
    start(&db, &p16, date(2027, 1, 1)).await.unwrap();
    let r = start(&db, &p15, date(2027, 6, 1)).await.unwrap();
    assert_eq!(r.steps, ["1.1.0 additive part"], "no removal: 1.6 ran here and may run again");
    assert_eq!(columns(&db, "t").await, ["id", "a", "b"]);
}

#[tokio::test]
async fn a_failed_part_is_rolled_back() {
    let dir = tempfile::tempdir().unwrap();
    let db = pool(dir.path()).await;
    let broken = "-- @additive\nCREATE TABLE u (id INTEGER PRIMARY KEY);\nINSERT INTO missing VALUES (1);\n";
    let dangling = "-- @additive\nCREATE TABLE v (t_id INTEGER REFERENCES t(id));\nINSERT INTO v VALUES (99);\n";
    start(&db, &program("1.0.0", "", &[("1.0.0.sql", BASE)]), 0).await.unwrap();
    let err =
        start(&db, &program("1.1.0", "1.0\n", &[("1.0.0.sql", BASE), ("1.1.0.sql", broken)]), 1).await.unwrap_err();
    assert!(matches!(&err, UpdateError::Failed { update, part: "additive", .. } if update == "1.1.0"), "{err}");
    assert!(columns(&db, "u").await.is_empty(), "rolled back");
    let err =
        start(&db, &program("1.1.0", "1.0\n", &[("1.0.0.sql", BASE), ("1.1.0.sql", dangling)]), 2).await.unwrap_err();
    assert!(err.to_string().contains("foreign keys"), "{err}");
    assert!(columns(&db, "v").await.is_empty());
    let recorded: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM musicus_updates").fetch_one(&db).await.unwrap();
    assert_eq!(recorded, 1);
    let fk: bool = sqlx::query_scalar("PRAGMA foreign_keys").fetch_one(&db).await.unwrap();
    assert!(fk, "foreign keys on again");
}

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap().to_path_buf()
}

/// Runs `scripts/release-migrations.py`; `None` if no Python is installed.
fn release_script(migrations: &Path, version: &str) -> Option<std::process::Output> {
    let script = repo().join("scripts/release-migrations.py");
    ["python3", "python"].iter().find_map(|py| {
        std::process::Command::new(py).arg(&script).arg("--migrations").arg(migrations).arg(version).output().ok()
    })
}

fn write(dir: &Path, files: &[(&str, &str)]) {
    for (name, text) in files {
        let path = dir.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
}

const P1: &str = "-- Adds table c.
-- @additive
CREATE TABLE c (id INTEGER PRIMARY KEY);
-- @removal
ALTER TABLE t DROP COLUMN a;
";
const P2: &str = "-- @additive\nCREATE TABLE d (id INTEGER PRIMARY KEY);\n-- @rebuild\nDELETE FROM d;\n";

/// R-OPS-023/R-OPS-024: development builds apply previews (never their
/// removal); the release script merges them into `<version>.sql`; a database
/// that applied all of them records the release update without running it.
#[tokio::test]
async fn previews_are_merged_and_recognised_by_the_release_update() {
    let src = tempfile::tempdir().unwrap();
    write(
        src.path(),
        &[("releases.txt", "1.0\n"), ("1.0.0.sql", BASE), ("preview/0001-c.sql", P1), ("preview/0002-d.sql", P2)],
    );
    let dev = Program::from_dir(src.path(), "1.0.0", false).unwrap();
    assert!(
        Program::from_dir(src.path(), "1.0.0", true)
            .unwrap()
            .check_build()
            .unwrap_err()
            .to_string()
            .contains("0002-d.sql")
    );

    let dir = tempfile::tempdir().unwrap();
    let db = pool(dir.path()).await;
    let r = start(&db, &dev, date(2027, 1, 1)).await.unwrap();
    assert_eq!(r.steps, ["1.0.0 additive part", "0001-c additive part", "0002-d additive part"]);
    assert_eq!(columns(&db, "t").await, ["id", "a"], "preview removals never run");
    // A database of a branch with only the first preview.
    let partial_dir = tempfile::tempdir().unwrap();
    let partial = pool(partial_dir.path()).await;
    let branch = tempfile::tempdir().unwrap();
    write(branch.path(), &[("releases.txt", "1.0\n"), ("1.0.0.sql", BASE), ("preview/0001-c.sql", P1)]);
    start(&partial, &Program::from_dir(branch.path(), "1.0.0", false).unwrap(), date(2027, 1, 1)).await.unwrap();
    // The other branch's program ignores the unknown preview.
    let r = start(&partial, &program("1.0.0", "1.0\n", &[("1.0.0.sql", BASE)]), date(2027, 1, 2)).await.unwrap();
    assert_eq!(r.ignored, ["0001-c"]);

    let Some(out) = release_script(src.path(), "1.1.0") else {
        eprintln!("no Python: release script not tested");
        return;
    };
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(!src.path().join("preview").exists(), "previews deleted");
    assert_eq!(std::fs::read_to_string(src.path().join("releases.txt")).unwrap(), "1.0\n1.1\n");
    let merged = std::fs::read_to_string(src.path().join("1.1.0.sql")).unwrap();
    assert!(merged.contains("-- @previews: 0001-c 0002-d\n"), "{merged}");
    let release = Program::from_dir(src.path(), "1.1.0", true).unwrap();
    release.check_build().unwrap();
    let u = release.updates.iter().find(|u| u.name == "1.1.0").unwrap();
    assert!(u.sections.additive.contains("CREATE TABLE c") && u.sections.additive.contains("CREATE TABLE d"));
    assert!(u.sections.rebuild.contains("DELETE FROM d") && u.sections.removal.contains("DROP COLUMN a"));

    // The developer's database: recorded from its previews, not run again (CREATE TABLE would fail).
    let r = start(&db, &release, date(2027, 2, 1)).await.unwrap();
    assert_eq!(r.steps, ["1.1.0 recorded from its previews"]);
    assert!(r.ignored.is_empty());
    let previews: String =
        sqlx::query_scalar("SELECT previews FROM musicus_updates WHERE name = '1.1.0'").fetch_one(&db).await.unwrap();
    assert_eq!(previews, r#"["0001-c","0002-d"]"#);
    // A fresh database runs the merged update.
    let fresh_dir = tempfile::tempdir().unwrap();
    let fresh = pool(fresh_dir.path()).await;
    let r = start(&fresh, &release, date(2027, 2, 1)).await.unwrap();
    assert_eq!(r.steps, ["1.0.0 additive part", "1.1.0 additive part"]);
    // The branch database that applied only some of the previews is unclear.
    let err = start(&partial, &release, date(2027, 2, 1)).await.unwrap_err();
    assert!(matches!(err, UpdateError::Unclear(_)), "{err}");
    assert!(err.to_string().contains("applied 0001-c but not 0002-d"), "{err}");

    // The merged removal runs once due, like any release update's.
    let later = program("1.5.0", LINES, &[]);
    let p15 = Program { updates: release.updates.clone(), ..later };
    let r = start(&db, &p15, date(2027, 9, 1)).await.unwrap();
    assert_eq!(r.steps, ["1.1.0 removal part"]);
    assert_eq!(columns(&db, "t").await, ["id"]);
}

#[test]
fn release_script_refuses_mistakes() {
    let src = tempfile::tempdir().unwrap();
    write(src.path(), &[("releases.txt", "1.0\n1.1\n"), ("1.0.0.sql", BASE), ("preview/0001-c.sql", P1)]);
    let Some(out) = release_script(src.path(), "1.1.1") else { return };
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("patch release"));
    let out = release_script(src.path(), "1.1.0").unwrap();
    assert!(String::from_utf8_lossy(&out.stderr).contains("already released"));
    let out = release_script(src.path(), "1.0.9").unwrap();
    assert!(!out.status.success());
    assert!(src.path().join("preview/0001-c.sql").exists(), "nothing changed");
    // A patch without previews changes nothing.
    std::fs::remove_file(src.path().join("preview/0001-c.sql")).unwrap();
    assert!(release_script(src.path(), "1.1.1").unwrap().status.success());
    assert_eq!(std::fs::read_to_string(src.path().join("releases.txt")).unwrap(), "1.0\n1.1\n");
    // A minor without previews only lists its line.
    assert!(release_script(src.path(), "1.2.0").unwrap().status.success());
    assert!(!src.path().join("1.2.0.sql").exists());
    assert_eq!(std::fs::read_to_string(src.path().join("releases.txt")).unwrap(), "1.0\n1.1\n1.2\n");
    // An existing, unreleased update file gets the previews appended.
    let first = tempfile::tempdir().unwrap();
    write(first.path(), &[("releases.txt", "# none yet\n"), ("1.0.0.sql", BASE), ("preview/0001-c.sql", P1)]);
    assert!(release_script(first.path(), "1.0.0").unwrap().status.success());
    let base = std::fs::read_to_string(first.path().join("1.0.0.sql")).unwrap();
    assert!(
        base.contains("-- @previews: 0001-c\n") && base.contains("CREATE TABLE t") && base.contains("CREATE TABLE c"),
        "{base}"
    );
    Program::from_dir(first.path(), "1.0.0", true).unwrap().check_build().unwrap();
}

/// The Python tests of the merge script and its simplifications (`scripts/test_release_migrations.py`).
#[test]
fn release_script_unit_tests() {
    let script = repo().join("scripts/test_release_migrations.py");
    let Some(out) =
        ["python3", "python"].iter().find_map(|py| std::process::Command::new(py).arg(&script).output().ok())
    else {
        eprintln!("no Python: release script not tested");
        return;
    };
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
}

/// The Python tests of the CI database check (`scripts/test_check_db_changes.py`, a-db-check, R-OPS-038);
/// they need `git`.
#[test]
fn db_check_script_unit_tests() {
    let script = repo().join("scripts/test_check_db_changes.py");
    let Some(out) =
        ["python3", "python"].iter().find_map(|py| std::process::Command::new(py).arg(&script).output().ok())
    else {
        eprintln!("no Python: database check script not tested");
        return;
    };
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
}

/// The release script on the real previews (as for 1.0.0): the merged, simplified update gives
/// the schema of the previews, and a database that applied them records it without running it.
#[tokio::test]
async fn merging_the_development_previews_keeps_the_schema() {
    let src = tempfile::tempdir().unwrap();
    let migrations = Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    std::fs::copy(migrations.join("releases.txt"), src.path().join("releases.txt")).unwrap();
    std::fs::create_dir(src.path().join("preview")).unwrap();
    for f in std::fs::read_dir(migrations.join("preview")).unwrap() {
        let f = f.unwrap();
        std::fs::copy(f.path(), src.path().join("preview").join(f.file_name())).unwrap();
    }
    let Some(out) = release_script(src.path(), "1.0.0") else { return };
    let log = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(log.contains("simplified (fold-add: column `reader_mode` into CREATE TABLE `users`)"), "{log}");
    assert!(log.contains("verified:"), "{log}");
    let release = Program::from_dir(src.path(), "1.0.0", true).unwrap();
    release.check_build().unwrap();

    let dev_dir = tempfile::tempdir().unwrap();
    let (dev, _) = Db::open(&dev_dir.path().join("musicus.sqlite"), 1).await.unwrap();
    let fresh_dir = tempfile::tempdir().unwrap();
    let fresh = pool(fresh_dir.path()).await;
    assert_eq!(start(&fresh, &release, 0).await.unwrap().steps, ["1.0.0 additive part"]);
    assert_eq!(schema(&fresh).await, schema(&dev.read).await);
    let r = start(&dev.write, &release, 0).await.unwrap();
    assert_eq!(r.steps, ["1.0.0 recorded from its previews"]);
}

/// R-OPS-038: `schema.sql` is the structure of a new database after all updates (a-db-check).
#[tokio::test]
async fn the_schema_snapshot_is_up_to_date() {
    let dir = tempfile::tempdir().unwrap();
    let (db, _) = Db::open(&dir.path().join("musicus.sqlite"), 1).await.unwrap();
    let got = updates::schema::snapshot(&db.read).await.unwrap();
    let want = include_str!("../schema.sql");
    assert!(
        got.contains("CREATE TABLE users (") && !got.contains("TABLE musicus_meta") && !got.contains("score_fts_data")
    );
    assert!(
        got == want,
        "crates/musicus-server/schema.sql is out of date: run scripts/db-schema.sh and commit it with the preview update"
    );
}

/// The SQL of every schema object, without comments, quotes and layout.
async fn schema(pool: &SqlitePool) -> Vec<(String, String, String)> {
    let rows: Vec<(String, String, Option<String>)> = sqlx::query_as(
        "SELECT type, name, sql FROM sqlite_master
         WHERE tbl_name NOT IN ('musicus_meta', 'musicus_updates', 'musicus_upgrades', '_sqlx_migrations')
         ORDER BY type, name",
    )
    .fetch_all(pool)
    .await
    .unwrap();
    rows.into_iter()
        .map(|(t, n, sql)| {
            let sql = sql.unwrap_or_default();
            let code: String =
                sql.lines().map(|l| l.split("--").next().unwrap_or("")).collect::<Vec<_>>().join(" ").replace('"', "");
            let mut norm = String::new();
            for word in code.split_whitespace() {
                let glue = norm.ends_with(['(', ',']) || word.starts_with([')', ',', '(']);
                if !norm.is_empty() && !glue {
                    norm.push(' ');
                }
                norm.push_str(word);
            }
            (t, n, norm)
        })
        .collect()
}

/// Development databases of the `sqlx::migrate!` time (0001–0010 applied)
/// are recorded once as having the previews 0001–0010; the previews (fresh
/// database) give exactly the schema of the sqlx migrations (only 0009 lost
/// its own PRAGMA/BEGIN/COMMIT); the data stays.
#[tokio::test]
async fn previews_give_the_schema_of_the_sqlx_migrations() {
    let legacy_dir = tempfile::tempdir().unwrap();
    let legacy = pool(legacy_dir.path()).await;
    sqlx::raw_sql(include_str!("fixtures/legacy-0001-0010.sql")).execute(&legacy).await.unwrap();
    sqlx::query("INSERT INTO groups (name) VALUES ('Trumpets')").execute(&legacy).await.unwrap();
    legacy.close().await;
    let (db, report) = Db::open(&legacy_dir.path().join("musicus.sqlite"), 2).await.unwrap();
    assert_eq!(report.steps.len(), 1, "{:?}", report.steps);
    assert!(
        report.steps[0].starts_with("recognised sqlx migrations 1-10 as the previews 0001-init, "),
        "{:?}",
        report.steps
    );
    assert!(report.other_ran_last, "derived data is rebuilt");
    let groups: Vec<String> = sqlx::query_scalar("SELECT name FROM groups").fetch_all(&db.read).await.unwrap();
    assert_eq!(groups, ["Trumpets"]);
    let rows: Vec<(String, String, String)> =
        sqlx::query_as("SELECT name, kind, applied_by FROM musicus_updates ORDER BY name")
            .fetch_all(&db.read)
            .await
            .unwrap();
    assert_eq!(rows.len(), 10);
    assert_eq!(rows[9], ("0010-german-key-spellings".into(), "preview".into(), "sqlx migration 10".into()));

    let fresh_dir = tempfile::tempdir().unwrap();
    let (fresh, report) = Db::open(&fresh_dir.path().join("musicus.sqlite"), 2).await.unwrap();
    assert_eq!(report.steps.len(), 10);
    assert_eq!(report.steps[0], "0001-init additive part");
    let (want, got) = (schema(&db.read).await, schema(&fresh.read).await);
    assert!(want.len() > 40, "{want:?}");
    for (w, g) in want.iter().zip(&got) {
        assert_eq!(g, w);
    }
    assert_eq!(got.len(), want.len());

    // Recognised only once: the next start is an ordinary one.
    db.write.close().await;
    let (_, report) = Db::open(&legacy_dir.path().join("musicus.sqlite"), 2).await.unwrap();
    assert!(report.steps.is_empty() && !report.other_ran_last);
}

/// A database that applied only sqlx migrations 1–8 gets previews 0009 and 0010.
#[tokio::test]
async fn an_older_development_database_gets_the_remaining_previews() {
    let dir = tempfile::tempdir().unwrap();
    let db = pool(dir.path()).await;
    let fixture = include_str!("fixtures/legacy-0001-0010.sql");
    let upto = fixture.find("\n-- ===== 0009_").unwrap();
    let bookkeeping = &fixture[fixture.find("\n-- ===== sqlx").unwrap()..];
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("{}\n{bookkeeping}", &fixture[..upto]))).execute(&db).await.unwrap();
    sqlx::query("DELETE FROM _sqlx_migrations WHERE version > 8").execute(&db).await.unwrap();
    db.close().await;
    let (_, report) = Db::open(&dir.path().join("musicus.sqlite"), 1).await.unwrap();
    assert_eq!(report.steps[1..], ["0009-i18n-names additive part", "0010-german-key-spellings additive part"]);
    assert!(report.steps[0].starts_with("recognised sqlx migrations 1-8 "), "{:?}", report.steps);
}

#[tokio::test]
async fn unclear_and_foreign_databases_are_refused() {
    // Development migrations other than exactly 0001–0010.
    let dir = tempfile::tempdir().unwrap();
    let db = pool(dir.path()).await;
    sqlx::raw_sql(include_str!("fixtures/legacy-0001-0010.sql")).execute(&db).await.unwrap();
    sqlx::query("DELETE FROM _sqlx_migrations WHERE version = 5").execute(&db).await.unwrap();
    db.close().await;
    let Err(err) = Db::open(&dir.path().join("musicus.sqlite"), 1).await else { panic!("opened") };
    assert!(err.to_string().contains("not a gapless") && err.to_string().contains("fresh demo database"), "{err}");
    // Tables, but nothing of Musicus.
    let dir = tempfile::tempdir().unwrap();
    let db = pool(dir.path()).await;
    sqlx::query("CREATE TABLE other (x)").execute(&db).await.unwrap();
    db.close().await;
    assert!(matches!(Db::open(&dir.path().join("musicus.sqlite"), 1).await, Err(UpdateError::Unrecognised)));
}
