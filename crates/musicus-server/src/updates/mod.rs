//! Database updates: the own update runner that replaced `sqlx::migrate!`
//! (d-update-model, a-preview-migrations, a-update-runner;
//! `docs/technical/chapters/14-updates.typ`).
//!
//! * `migrations/<version>.sql` are release updates, `migrations/preview/NNNN-<topic>.sql`
//!   preview updates of the development line (format: [`file`]). Both are embedded at
//!   build time (`build.rs`); `migrations/releases.txt` lists the released lines in
//!   release order.
//! * The database records what ran in the marker tables `musicus_meta`,
//!   `musicus_updates` and `musicus_upgrades` (see [`runner`]).
//! * Start: [`Program::check_build`], [`inspect`] (reads only; refuses outside the
//!   window), then [`apply`]. `Db::open` runs the three in that order; a snapshot
//!   before `apply` goes between them.

pub mod file;
pub mod runner;
pub mod window;

pub use runner::{Plan, Report, apply, inspect};
pub use window::{Line, Version};

use std::{fmt, sync::OnceLock};

/// The previews that the development migrations 0001–0010 of the
/// `sqlx::migrate!` time became (text unchanged, a-update-runner); a database
/// that applied sqlx migrations 1..k is recorded once as having the first k.
pub const LEGACY_PREVIEWS: [&str; 10] = [
    "0001-init",
    "0002-part-names",
    "0003-system-layouts",
    "0004-layout-corrections",
    "0005-part-voices",
    "0006-archive-number",
    "0007-import",
    "0008-import-file-problem",
    "0009-i18n-names",
    "0010-german-key-spellings",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Release,
    Preview,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Release => "release",
            Kind::Preview => "preview",
        }
    }
}

/// One database update known to this program.
#[derive(Clone, Debug)]
pub struct Update {
    /// `1.2.0` for a release update, `0003-topic` for a preview.
    pub name: &'static str,
    pub kind: Kind,
    /// The release that brings it; for a preview the program's own version.
    pub release: Version,
    /// Path below `migrations/`, for messages.
    pub file: String,
    pub sections: file::Sections,
}

/// Everything the runner needs to know about the running program.
#[derive(Clone, Debug)]
pub struct Program {
    pub version: Version,
    /// Released lines in release order (`releases.txt`).
    pub releases: Vec<Line>,
    /// Release updates by version, then previews by name.
    pub updates: Vec<Update>,
    /// Built by the release workflow (`MUSICUS_RELEASE_BUILD=1` at build time).
    pub release_build: bool,
}

mod embedded {
    include!(concat!(env!("OUT_DIR"), "/updates.rs"));
}

impl Program {
    /// The updates embedded in this binary.
    pub fn builtin() -> Result<&'static Program, UpdateError> {
        static PROGRAM: OnceLock<Result<Program, String>> = OnceLock::new();
        PROGRAM
            .get_or_init(|| {
                let release_build = matches!(option_env!("MUSICUS_RELEASE_BUILD"), Some("1" | "true" | "yes"));
                let files = embedded::FILES.iter().map(|(path, text)| (path.to_string(), *text)).collect();
                Program::new(env!("CARGO_PKG_VERSION"), embedded::RELEASES, files, release_build)
            })
            .as_ref()
            .map_err(|e| UpdateError::Invalid(e.clone()))
    }

    /// Reads the updates from a `migrations` folder at run time (tests, tools).
    /// The texts are leaked: SQL must be `&'static str` (R-SEC-006).
    pub fn from_dir(dir: &std::path::Path, version: &str, release_build: bool) -> Result<Program, String> {
        let read = |p: &std::path::Path| std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()));
        let releases = match dir.join("releases.txt") {
            p if p.exists() => read(&p)?,
            _ => String::new(),
        };
        let mut files = Vec::new();
        for (sub, prefix) in [(dir.to_path_buf(), ""), (dir.join("preview"), "preview/")] {
            let Ok(entries) = std::fs::read_dir(&sub) else { continue };
            for entry in entries {
                let path = entry.map_err(|e| e.to_string())?.path();
                if path.extension().is_some_and(|e| e == "sql") {
                    let name = format!("{prefix}{}", path.file_name().unwrap_or_default().to_string_lossy());
                    files.push((name, &*Box::leak(read(&path)?.into_boxed_str())));
                }
            }
        }
        Program::new(version, Box::leak(releases.into_boxed_str()), files, release_build)
    }

    /// `files`: paths below `migrations/` (`1.0.0.sql`, `preview/0001-x.sql`) with their text.
    pub fn new(
        version: &str,
        releases: &str,
        files: Vec<(String, &'static str)>,
        release_build: bool,
    ) -> Result<Program, String> {
        let version = Version::parse(version).ok_or_else(|| format!("program version `{version}` is not X.Y.Z"))?;
        let releases = window::parse_releases(releases)?;
        let mut updates = Vec::new();
        for (path, text) in files {
            let sections = file::parse(&path, text)?;
            let (kind, stem) = match path.strip_prefix("preview/") {
                Some(p) => (Kind::Preview, p),
                None => (Kind::Release, path.as_str()),
            };
            let stem = stem.strip_suffix(".sql").ok_or_else(|| format!("{path}: not an .sql file"))?;
            let name: &'static str = Box::leak(stem.to_owned().into_boxed_str());
            let release = match kind {
                Kind::Release => {
                    let v = Version::parse(name)
                        .filter(|v| v.to_string() == name)
                        .ok_or_else(|| format!("{path}: a release update is named after its release (`1.2.0.sql`)"))?;
                    if v.patch != 0 {
                        return Err(format!("{path}: patch releases bring no database update (R-OPS-018)"));
                    }
                    v
                }
                Kind::Preview => {
                    if !is_preview_name(name) {
                        return Err(format!("{path}: a preview is named `NNNN-topic.sql` (lower case, digits, `-`)"));
                    }
                    if !sections.previews.is_empty() {
                        return Err(format!("{path}: only release updates list previews"));
                    }
                    version
                }
            };
            for p in &sections.previews {
                if !is_preview_name(p) {
                    return Err(format!("{path}: `{p}` is not a preview name"));
                }
            }
            updates.push(Update { name, kind, release, file: path, sections });
        }
        updates.sort_by(|a, b| match (a.kind, b.kind) {
            (Kind::Release, Kind::Release) => a.release.cmp(&b.release),
            (Kind::Release, Kind::Preview) => std::cmp::Ordering::Less,
            (Kind::Preview, Kind::Release) => std::cmp::Ordering::Greater,
            (Kind::Preview, Kind::Preview) => a.name.cmp(b.name),
        });
        let mut seen = std::collections::HashSet::new();
        for u in &updates {
            for n in std::iter::once(&u.name).chain(&u.sections.previews) {
                if !seen.insert(*n) {
                    return Err(format!("{}: `{n}` exists twice (a merged preview must be deleted)", u.file));
                }
            }
        }
        Ok(Program { version, releases, updates, release_build })
    }

    /// R-OPS-024: a release build refuses to start with preview updates.
    pub fn check_build(&self) -> Result<(), UpdateError> {
        let previews: Vec<String> =
            self.updates.iter().filter(|u| u.kind == Kind::Preview).map(|u| format!("migrations/{}", u.file)).collect();
        if self.release_build && !previews.is_empty() {
            return Err(UpdateError::PreviewsInReleaseBuild(previews));
        }
        Ok(())
    }

    /// Names this program knows: its updates and the previews merged into them.
    pub fn known(&self) -> Vec<&'static str> {
        let mut names: Vec<&'static str> = self
            .updates
            .iter()
            .flat_map(|u| std::iter::once(u.name).chain(u.sections.previews.iter().copied()))
            .collect();
        names.sort_unstable();
        names
    }
}

fn is_preview_name(name: &str) -> bool {
    let b = name.as_bytes();
    b.len() > 5
        && b[..4].iter().all(u8::is_ascii_digit)
        && b[4] == b'-'
        && b[5..].iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

/// Why the program does not start on a database. The texts are for the
/// administrator reading the log (R-OPS-015), not for the web client.
pub enum UpdateError {
    /// A release build contains previews (R-OPS-024).
    PreviewsInReleaseBuild(Vec<String>),
    /// Outside the downgrade window (R-OPS-015); nothing was written.
    TooOld {
        own: Version,
        last: Option<String>,
        oldest: Version,
    },
    /// A development database that cannot be matched with this program's updates.
    Unclear(String),
    /// Tables, but neither a marker nor the development migrations 0001–0010.
    Unrecognised,
    /// The embedded update files are broken (a build mistake).
    Invalid(String),
    /// A part failed and was rolled back.
    Failed {
        update: String,
        part: &'static str,
        source: sqlx::Error,
    },
    Db(sqlx::Error),
}

impl fmt::Display for UpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UpdateError::PreviewsInReleaseBuild(files) => write!(
                f,
                "This release build contains preview database updates ({}). Merge them into the release update \
                 with scripts/release-migrations.py before building a release.",
                files.join(", ")
            ),
            UpdateError::TooOld { own, last: Some(last), oldest } => write!(
                f,
                "This data was last used by Musicus {last}. Musicus {own} can no longer use it; \
                 start Musicus {oldest} or newer."
            ),
            UpdateError::TooOld { own, last: None, oldest } => {
                write!(f, "This data needs Musicus {oldest} or newer; Musicus {own} cannot use it.")
            }
            UpdateError::Unclear(detail) => write!(
                f,
                "The database is a development database in an unclear state: {detail}. \
                 Start with a fresh demo database (move the data directory away)."
            ),
            UpdateError::Unrecognised => {
                write!(f, "The database has tables but no Musicus version marker; it was not written by this program.")
            }
            UpdateError::Invalid(e) => write!(f, "The database updates of this program are invalid: {e}"),
            UpdateError::Failed { update, part, source } => {
                write!(f, "The database update {update} failed in its {part} part and was rolled back: {source}")
            }
            UpdateError::Db(e) => write!(f, "Reading the database version marker failed: {e}"),
        }
    }
}

/// Like `Display`: `main` prints the error with `{:?}`, and this text is the
/// last line an administrator sees (R-OPS-015).
impl fmt::Debug for UpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl std::error::Error for UpdateError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            UpdateError::Failed { source, .. } | UpdateError::Db(source) => Some(source),
            _ => None,
        }
    }
}

impl From<sqlx::Error> for UpdateError {
    fn from(e: sqlx::Error) -> Self {
        UpdateError::Db(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_updates_are_valid() {
        let p = Program::builtin().unwrap();
        // Until a release merges them, the development migrations are previews.
        for name in LEGACY_PREVIEWS {
            assert!(p.known().contains(&name), "{name} is a preview or merged into a release update");
        }
        if p.updates.iter().all(|u| u.kind == Kind::Release) {
            p.check_build().unwrap();
        }
    }

    #[test]
    fn file_names_are_checked() {
        let new = |files: &[(&str, &'static str)]| {
            Program::new("1.1.0", "1.0\n", files.iter().map(|(p, t)| (p.to_string(), *t)).collect(), false)
        };
        let a = "-- @additive\n";
        assert!(new(&[("1.0.1.sql", a)]).unwrap_err().contains("R-OPS-018"));
        assert!(new(&[("init.sql", a)]).unwrap_err().contains("named after its release"));
        assert!(new(&[("1.0.sql", a)]).is_err());
        assert!(new(&[("preview/1-x.sql", a)]).unwrap_err().contains("NNNN-topic"));
        assert!(new(&[("preview/0001-Big.sql", a)]).is_err());
        assert!(new(&[("preview/0001-a.sql", "-- @previews: 0002-b\n-- @additive\n")]).is_err());
        assert!(
            new(&[("1.1.0.sql", "-- @previews: 0001-a\n-- @additive\n"), ("preview/0001-a.sql", a)])
                .unwrap_err()
                .contains("twice")
        );
        let p =
            new(&[("preview/0002-b.sql", a), ("1.1.0.sql", a), ("preview/0001-a.sql", a), ("1.0.0.sql", a)]).unwrap();
        let names: Vec<_> = p.updates.iter().map(|u| u.name).collect();
        assert_eq!(names, ["1.0.0", "1.1.0", "0001-a", "0002-b"]);
        assert!(!p.release_build);
    }

    #[test]
    fn release_builds_refuse_previews() {
        let files =
            vec![("1.0.0.sql".to_owned(), "-- @additive\n"), ("preview/0001-a.sql".to_owned(), "-- @additive\n")];
        let dev = Program::new("1.0.0", "", files.clone(), false).unwrap();
        dev.check_build().unwrap();
        let release = Program::new("1.0.0", "", files, true).unwrap();
        let msg = release.check_build().unwrap_err().to_string();
        assert!(msg.contains("migrations/preview/0001-a.sql"), "{msg}");
    }
}
