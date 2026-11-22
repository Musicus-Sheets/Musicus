//! Reads the version marker, plans and runs the parts of the database
//! updates (d-update-model, a-update-runner).
//!
//! Marker tables (created by the runner, `marker_format` 1; later formats
//! only add columns and keys, so every release from 1.0 on can read them):
//!
//! * `musicus_meta (key, value)`: `marker_format`, `last_version` (the program
//!   that last started), `last_updates` (JSON list of the update and preview
//!   names that program knew), `oldest_allowed` (set when a removal runs).
//! * `musicus_updates`: one row per applied update: `name`, `kind`
//!   (`release`/`preview`), `release`, `additive_at`, `rebuilt_at`,
//!   `removal_at` (NULL while the removal is pending or the update has none),
//!   `previews` (JSON list merged into it), `applied_by`.
//! * `musicus_upgrades (from_version, to_version, at)`: every start of another
//!   version than the one that ran last; source of rule (b) of the window.

use super::{
    BASELINE, Kind, Program, Update, UpdateError,
    file::is_empty,
    window::{Upgrade, Version, Window},
};
use sqlx::{Connection, SqliteConnection, SqlitePool};
use std::collections::{HashMap, HashSet};

/// Version of the marker tables written by this runner.
pub const MARKER_FORMAT: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Start {
    /// No tables at all: everything runs.
    Fresh,
    /// A development database of the `sqlx::migrate!` time with 0001–0010
    /// applied: recorded once as having the baseline.
    Legacy,
    Marked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    /// Refill the new structures of an update from the old ones.
    Rebuild(usize),
    Additive(usize),
    /// A release update whose previews all ran: recorded without running.
    Merged(usize),
    Removal(usize),
}

/// What a start will do, computed without writing anything ([`inspect`]).
#[derive(Debug)]
pub struct Plan {
    start: Start,
    steps: Vec<Step>,
    /// `last_version` as stored, if the program version differs from it.
    upgrade_from: Option<String>,
    /// Applied updates this program does not know (a newer release or another branch wrote them).
    pub ignored: Vec<String>,
    /// Another program (version or set of updates) used the database last:
    /// derived data is rebuilt (search index).
    pub other_ran_last: bool,
    oldest_allowed: Option<Version>,
}

impl Plan {
    /// True if the start changes more than the marker's `last_*` keys: the
    /// moment to write a snapshot first (R-OPS-019, PR D).
    pub fn has_work(&self) -> bool {
        !self.steps.is_empty() || self.start == Start::Legacy
    }

    /// One line per step, for the log.
    pub fn describe(&self, program: &Program) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        if self.start == Start::Legacy {
            out.push(format!("recognised development migrations 0001-0010 as update {BASELINE}"));
        }
        for s in &self.steps {
            let (what, i) = match *s {
                Step::Rebuild(i) => ("rebuild", i),
                Step::Additive(i) => ("additive part", i),
                Step::Merged(i) => ("recorded from its previews", i),
                Step::Removal(i) => ("removal part", i),
            };
            out.push(format!("{} {what}", program.updates[i].name));
        }
        out
    }
}

/// What a start did.
#[derive(Debug, Default)]
pub struct Report {
    pub steps: Vec<String>,
    pub upgraded_from: Option<String>,
    pub ignored: Vec<String>,
    pub other_ran_last: bool,
}

struct Applied {
    kind: String,
    previews: Vec<String>,
    removed: bool,
}

struct Marker {
    last_version: Option<String>,
    last_updates: Option<Vec<String>>,
    oldest_allowed: Option<Version>,
    applied: HashMap<String, Applied>,
    upgrades: Vec<(String, String, i64)>,
}

/// Step 1 of the start: reads the marker and plans the work. Writes nothing;
/// refuses (R-OPS-015) when the program is older than `oldest_allowed`.
pub async fn inspect(pool: &SqlitePool, program: &Program, now: i64) -> Result<Plan, UpdateError> {
    let mut conn = pool.acquire().await?;
    let tables: Vec<String> =
        sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'")
            .fetch_all(&mut *conn)
            .await?;
    let has = |t: &str| tables.iter().any(|n| n == t);
    let (start, marker) = if has("musicus_meta") {
        (Start::Marked, read_marker(&mut conn).await?)
    } else if has("_sqlx_migrations") {
        check_legacy(&mut conn).await?;
        let mut applied = HashMap::new();
        applied.insert(BASELINE.to_owned(), Applied { kind: "release".into(), previews: vec![], removed: false });
        (Start::Legacy, Marker { applied, ..Marker::empty() })
    } else if tables.is_empty() {
        (Start::Fresh, Marker::empty())
    } else {
        return Err(UpdateError::Unrecognised);
    };
    if start != Start::Fresh && program.updates.first().is_none_or(|u| u.name != BASELINE) {
        return Err(UpdateError::Invalid(format!("the first update must be the baseline {BASELINE}")));
    }

    let own = program.version;
    if let Some(oldest) = marker.oldest_allowed
        && own.line() < oldest.line()
    {
        return Err(UpdateError::TooOld { own, last: marker.last_version.clone(), oldest });
    }

    let known: HashSet<&str> = program.known().into_iter().collect();
    let mut ignored: Vec<String> = marker.applied.keys().filter(|n| !known.contains(n.as_str())).cloned().collect();
    ignored.sort();

    let own_text = own.to_string();
    let upgrade_from = marker.last_version.clone().filter(|lv| *lv != own_text);

    // Removals only by the newest program that ever ran here, with nothing unknown applied.
    let mut versions: Vec<&str> = marker.last_version.iter().map(String::as_str).collect();
    versions.extend(marker.upgrades.iter().flat_map(|(f, t, _)| [f.as_str(), t.as_str()]));
    let parsed: Option<Vec<Version>> = versions.iter().map(|v| Version::parse(v)).collect();
    // The upgrade this start performs counts from now (rule b).
    let pending = upgrade_from.as_ref().map(|f| (f.clone(), own_text.clone(), now));
    let upgrades: Option<Vec<Upgrade>> = marker
        .upgrades
        .iter()
        .chain(pending.as_ref())
        .map(|(f, t, at)| Some(Upgrade { from: Version::parse(f)?, to: Version::parse(t)?, at: *at }))
        .collect();
    let window = match (parsed, upgrades) {
        (Some(seen), Some(upgrades)) if ignored.is_empty() && seen.iter().all(|v| v.line() <= own.line()) => {
            Some(Window::new(&program.releases, own.line(), &upgrades, now))
        }
        _ => None,
    };
    let due = |u: &Update| {
        u.kind == Kind::Release
            && !is_empty(u.sections.removal)
            && window.as_ref().is_some_and(|w| w.floor() >= u.release.line())
    };

    let last_knew = |name: &str| marker.last_updates.as_ref().is_none_or(|l| l.iter().any(|n| n == name));
    let mut rebuilds = Vec::new();
    let mut additions = Vec::new();
    let mut removals = Vec::new();
    for (i, u) in program.updates.iter().enumerate() {
        let applied_previews: Vec<&str> =
            u.sections.previews.iter().copied().filter(|p| marker.applied.contains_key(*p)).collect();
        let pending_removal = match marker.applied.get(u.name) {
            Some(row) => {
                let missing: Vec<&str> = u
                    .sections
                    .previews
                    .iter()
                    .copied()
                    .filter(|p| !row.previews.iter().any(|r| r == p) && !applied_previews.contains(p))
                    .collect();
                if !missing.is_empty() {
                    return Err(UpdateError::Unclear(format!(
                        "update {} ran before the previews {} were merged into it",
                        u.name,
                        missing.join(", ")
                    )));
                }
                if row.kind != u.kind.as_str() {
                    return Err(UpdateError::Unclear(format!("{} is recorded as a {} update", u.name, row.kind)));
                }
                !row.removed
            }
            None if !u.sections.previews.is_empty() && !applied_previews.is_empty() => {
                if applied_previews.len() < u.sections.previews.len() {
                    let missing: Vec<&str> =
                        u.sections.previews.iter().copied().filter(|p| !applied_previews.contains(p)).collect();
                    return Err(UpdateError::Unclear(format!(
                        "of the previews merged into {} it applied {} but not {}",
                        u.name,
                        applied_previews.join(", "),
                        missing.join(", ")
                    )));
                }
                additions.push(Step::Merged(i));
                true
            }
            None => {
                additions.push(Step::Additive(i));
                if due(u) {
                    removals.push(Step::Removal(i));
                }
                continue;
            }
        };
        if !pending_removal {
            continue;
        }
        let knew =
            last_knew(u.name) || (!u.sections.previews.is_empty() && u.sections.previews.iter().all(|p| last_knew(p)));
        if !knew && !is_empty(u.sections.rebuild) {
            rebuilds.push(Step::Rebuild(i));
        }
        if due(u) {
            removals.push(Step::Removal(i));
        }
    }

    let mut own_known: Vec<&str> = known.iter().copied().collect();
    own_known.sort_unstable();
    let other_ran_last = start == Start::Legacy
        || upgrade_from.is_some()
        || marker.last_updates.as_ref().is_some_and(|l| {
            let mut l: Vec<&str> = l.iter().map(String::as_str).collect();
            l.sort_unstable();
            l != own_known
        });
    let mut oldest_allowed = marker.oldest_allowed;
    for s in &removals {
        if let Step::Removal(i) = s {
            let r = program.updates[*i].release;
            oldest_allowed = Some(oldest_allowed.map_or(r, |o| o.max(r)));
        }
    }
    let mut steps = rebuilds;
    steps.extend(additions);
    steps.extend(removals);
    Ok(Plan { start, steps, upgrade_from, ignored, other_ran_last, oldest_allowed })
}

impl Marker {
    fn empty() -> Self {
        Marker {
            last_version: None,
            last_updates: None,
            oldest_allowed: None,
            applied: HashMap::new(),
            upgrades: vec![],
        }
    }
}

async fn read_marker(conn: &mut SqliteConnection) -> Result<Marker, UpdateError> {
    let meta: HashMap<String, String> =
        sqlx::query_as("SELECT key, value FROM musicus_meta").fetch_all(&mut *conn).await?.into_iter().collect();
    let rows: Vec<(String, String, String, bool)> =
        sqlx::query_as("SELECT name, kind, previews, removal_at IS NOT NULL FROM musicus_updates")
            .fetch_all(&mut *conn)
            .await?;
    let applied = rows
        .into_iter()
        .map(|(name, kind, previews, removed)| {
            (name, Applied { kind, previews: serde_json::from_str(&previews).unwrap_or_default(), removed })
        })
        .collect();
    let upgrades = sqlx::query_as("SELECT from_version, to_version, at FROM musicus_upgrades ORDER BY at")
        .fetch_all(&mut *conn)
        .await?;
    let oldest_allowed = match meta.get("oldest_allowed") {
        Some(v) => {
            Some(Version::parse(v).ok_or_else(|| UpdateError::Unclear(format!("oldest_allowed `{v}` is no version")))?)
        }
        None => None,
    };
    Ok(Marker {
        last_version: meta.get("last_version").cloned(),
        last_updates: meta.get("last_updates").map(|l| serde_json::from_str(l).unwrap_or_default()),
        oldest_allowed,
        applied,
        upgrades,
    })
}

/// The development migrations 0001–0010 of the `sqlx::migrate!` time, all
/// applied successfully, nothing else: the schema of the baseline.
async fn check_legacy(conn: &mut SqliteConnection) -> Result<(), UpdateError> {
    let rows: Vec<(i64, bool)> =
        sqlx::query_as("SELECT version, success FROM _sqlx_migrations ORDER BY version").fetch_all(&mut *conn).await?;
    if rows.iter().map(|(v, ok)| (*v, *ok)).eq((1..=10).map(|v| (v, true))) {
        return Ok(());
    }
    let list: Vec<String> =
        rows.iter().map(|(v, ok)| format!("{v:04}{}", if *ok { "" } else { " (failed)" })).collect();
    Err(UpdateError::Unclear(format!(
        "it applied the development migrations [{}], not exactly 0001-0010",
        list.join(", ")
    )))
}

const CREATE_MARKER: &str = "
CREATE TABLE IF NOT EXISTS musicus_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS musicus_updates (
    name        TEXT PRIMARY KEY,
    kind        TEXT NOT NULL,
    release     TEXT NOT NULL,
    additive_at INTEGER NOT NULL,
    rebuilt_at  INTEGER,
    removal_at  INTEGER,
    previews    TEXT NOT NULL DEFAULT '[]',
    applied_by  TEXT NOT NULL
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS musicus_upgrades (
    id           INTEGER PRIMARY KEY,
    from_version TEXT NOT NULL,
    to_version   TEXT NOT NULL,
    at           INTEGER NOT NULL
);
INSERT OR IGNORE INTO musicus_meta (key, value) VALUES ('marker_format', '1');";

const SET_META: &str =
    "INSERT INTO musicus_meta (key, value) VALUES (?1, ?2) ON CONFLICT (key) DO UPDATE SET value = excluded.value";

const INSERT_UPDATE: &str = "INSERT INTO musicus_updates (name, kind, release, additive_at, previews, applied_by)
     VALUES (?1, ?2, ?3, ?4, ?5, ?6)";

/// Runs the plan: each part in its own transaction together with its record,
/// foreign keys off during the part and checked before the commit (SQLite's
/// table rebuild procedure). Finally writes `last_version`, `last_updates`
/// and, for a change of version, a row in `musicus_upgrades`.
pub async fn apply(pool: &SqlitePool, program: &Program, plan: &Plan, now: i64) -> Result<Report, UpdateError> {
    let mut conn = pool.acquire().await?;
    let own = program.version.to_string();
    {
        let mut tx = conn.begin().await?;
        sqlx::raw_sql(CREATE_MARKER).execute(&mut *tx).await?;
        if plan.start == Start::Legacy {
            sqlx::query(INSERT_UPDATE)
                .bind(BASELINE)
                .bind(Kind::Release.as_str())
                .bind(BASELINE)
                .bind(now)
                .bind("[]")
                .bind("sqlx migrations 0001-0010")
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
    }
    for step in &plan.steps {
        let (i, part) = match *step {
            Step::Rebuild(i) => (i, "rebuild"),
            Step::Additive(i) | Step::Merged(i) => (i, "additive"),
            Step::Removal(i) => (i, "removal"),
        };
        sqlx::query("PRAGMA foreign_keys = OFF").execute(&mut *conn).await?;
        let result = run_step(&mut conn, program, plan, *step, now).await;
        let on = sqlx::query("PRAGMA foreign_keys = ON").execute(&mut *conn).await;
        result.map_err(|source| UpdateError::Failed { update: program.updates[i].name.to_owned(), part, source })?;
        on?;
    }
    let mut tx = conn.begin().await?;
    let known = serde_json::to_string(&program.known()).expect("names serialise");
    sqlx::query(SET_META).bind("last_version").bind(&own).execute(&mut *tx).await?;
    sqlx::query(SET_META).bind("last_updates").bind(known).execute(&mut *tx).await?;
    if let Some(from) = &plan.upgrade_from {
        sqlx::query("INSERT INTO musicus_upgrades (from_version, to_version, at) VALUES (?1, ?2, ?3)")
            .bind(from)
            .bind(&own)
            .bind(now)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(Report {
        steps: plan.describe(program),
        upgraded_from: plan.upgrade_from.clone(),
        ignored: plan.ignored.clone(),
        other_ran_last: plan.other_ran_last,
    })
}

/// One part and its record in one transaction; foreign keys are off
/// (set by the caller outside the transaction) and checked before the commit.
async fn run_step(
    conn: &mut SqliteConnection,
    program: &Program,
    plan: &Plan,
    step: Step,
    now: i64,
) -> sqlx::Result<()> {
    let (i, sql) = match step {
        Step::Rebuild(i) => (i, program.updates[i].sections.rebuild),
        Step::Additive(i) => (i, program.updates[i].sections.additive),
        Step::Merged(i) => (i, ""),
        Step::Removal(i) => (i, program.updates[i].sections.removal),
    };
    let u = &program.updates[i];
    let mut tx = conn.begin().await?;
    if !is_empty(sql) {
        sqlx::raw_sql(sql).execute(&mut *tx).await?;
    }
    match step {
        Step::Additive(_) | Step::Merged(_) => {
            let previews = serde_json::to_string(&u.sections.previews).expect("names serialise");
            sqlx::query(INSERT_UPDATE)
                .bind(u.name)
                .bind(u.kind.as_str())
                .bind(u.release.to_string())
                .bind(now)
                .bind(previews)
                .bind(program.version.to_string())
                .execute(&mut *tx)
                .await?;
        }
        Step::Rebuild(_) => {
            sqlx::query("UPDATE musicus_updates SET rebuilt_at = ?2 WHERE name = ?1")
                .bind(u.name)
                .bind(now)
                .execute(&mut *tx)
                .await?;
        }
        Step::Removal(_) => {
            sqlx::query("UPDATE musicus_updates SET removal_at = ?2 WHERE name = ?1")
                .bind(u.name)
                .bind(now)
                .execute(&mut *tx)
                .await?;
            let oldest = plan.oldest_allowed.expect("planned with the removal").to_string();
            sqlx::query(SET_META).bind("oldest_allowed").bind(oldest).execute(&mut *tx).await?;
        }
    }
    let broken: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pragma_foreign_key_check").fetch_one(&mut *tx).await?;
    if broken > 0 {
        return Err(sqlx::Error::Protocol(format!("{broken} rows violate foreign keys")));
    }
    tx.commit().await
}
