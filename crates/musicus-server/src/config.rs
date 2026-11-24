//! Command line and environment configuration.

use clap::{Args, Parser, Subcommand, ValueEnum};
use std::{
    ffi::OsString,
    net::SocketAddr,
    path::{Path, PathBuf},
};

#[derive(Parser, Debug)]
#[command(name = "musicus-server", version = musicus_api::VERSION, about = "Sheet music library with private annotations")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Run the server.
    Serve(ServeArgs),
    /// Create an admin account and print its one-time invite link.
    CreateAdmin {
        #[command(flatten)]
        data: DataArgs,
        #[arg(long)]
        username: String,
        #[arg(long)]
        display_name: Option<String>,
        /// Base URL used to print the invite link.
        #[arg(long, env = "MUSICUS_PUBLIC_URL", default_value = "http://localhost:8080")]
        public_url: String,
    },
    /// Write a consistent copy of the database as it is (safe while the
    /// server runs; never updates the database, works with data of any
    /// release). Back up the `blobs` directory separately, after the database.
    Backup {
        #[command(flatten)]
        data: DataArgs,
        /// Target file; must not exist yet.
        #[arg(long)]
        to: PathBuf,
    },
    /// Replace the database with a backup or snapshot (the server must be
    /// stopped). The replaced database is kept in `snapshots/`; `blobs` is
    /// not changed. The next start updates the restored database as usual.
    Restore {
        #[command(flatten)]
        data: DataArgs,
        /// A file written by `backup`, or a snapshot from `snapshots/`.
        #[arg(long)]
        from: PathBuf,
    },
    /// After a failed database update: let the same program try the update
    /// again at its next start (removes `update-failed.json`).
    RetryUpdate {
        #[command(flatten)]
        data: DataArgs,
    },
    /// Exit 0 if the server on this machine answers `/api/health` (for
    /// container health checks; plain HTTP only).
    Healthcheck {
        #[arg(long, env = "MUSICUS_LISTEN", default_value = "[::]:8080")]
        listen: SocketAddr,
    },
}

#[derive(Args, Debug, Clone)]
pub struct DataArgs {
    /// Directory holding `musicus.sqlite` and the `blobs` file store.
    /// Default: the usual folder for application data of the system
    /// (Linux `$XDG_DATA_HOME/musicus` or `~/.local/share/musicus`, macOS
    /// `~/Library/Application Support/Musicus`, Windows `%APPDATA%\Musicus`).
    #[arg(long, env = "MUSICUS_DATA_DIR")]
    pub data_dir: Option<PathBuf>,
}

impl DataArgs {
    /// The data directory to use (R-OPS-031, a-data-dir-default).
    pub fn resolve(&self) -> Result<DataDir, String> {
        let legacy_db = Path::new(LEGACY_DATA_DIR).join("musicus.sqlite").is_file();
        resolve_data_dir(self.data_dir.clone(), Os::current(), &|key| std::env::var_os(key), legacy_db)
    }
}

/// The default data directory before R-OPS-031, relative to the working
/// directory. Still used when it holds a database and nothing is configured.
pub const LEGACY_DATA_DIR: &str = "data";

/// The operating system family whose conventions pick the default data folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    /// Linux and every other Unix except macOS (XDG base directories).
    Linux,
    MacOs,
    Windows,
}

impl Os {
    pub fn current() -> Self {
        if cfg!(windows) {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::MacOs
        } else {
            Os::Linux
        }
    }
}

/// Where the data directory came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataDirSource {
    /// `--data-dir` or `MUSICUS_DATA_DIR`.
    Configured,
    /// Nothing configured, but `./data` holds a database (the old default).
    Legacy,
    /// Nothing configured: the usual folder of the system.
    SystemDefault,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataDir {
    pub path: PathBuf,
    pub source: DataDirSource,
}

/// The usual folder for application data of `os`, read from the environment
/// (`env` is `std::env::var_os` outside tests). `None` when the variable it
/// needs (`HOME`, `APPDATA`) is missing.
pub fn default_data_dir(os: Os, env: &dyn Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let var = |key: &str| env(key).filter(|v| !v.is_empty()).map(PathBuf::from);
    match os {
        Os::Windows => var("APPDATA").map(|d| d.join("Musicus")),
        Os::MacOs => var("HOME").map(|h| h.join("Library").join("Application Support").join("Musicus")),
        // The XDG specification says to ignore relative paths.
        Os::Linux => var("XDG_DATA_HOME")
            .filter(|d| d.is_absolute())
            .or_else(|| var("HOME").map(|h| h.join(".local").join("share")))
            .map(|d| d.join("musicus")),
    }
}

/// Configured directory, else `./data` if it holds a database already (so
/// existing installations started from their folder keep their data), else
/// the usual folder of the system.
pub fn resolve_data_dir(
    configured: Option<PathBuf>,
    os: Os,
    env: &dyn Fn(&str) -> Option<OsString>,
    legacy_db_exists: bool,
) -> Result<DataDir, String> {
    if let Some(path) = configured {
        return Ok(DataDir { path, source: DataDirSource::Configured });
    }
    if legacy_db_exists {
        return Ok(DataDir { path: PathBuf::from(LEGACY_DATA_DIR), source: DataDirSource::Legacy });
    }
    match default_data_dir(os, env) {
        Some(path) => Ok(DataDir { path, source: DataDirSource::SystemDefault }),
        None => Err(format!(
            "cannot find the usual data folder of this system ({} is not set); choose one with --data-dir or MUSICUS_DATA_DIR",
            if os == Os::Windows { "APPDATA" } else { "HOME" }
        )),
    }
}

#[derive(Args, Debug, Clone)]
pub struct ServeArgs {
    #[command(flatten)]
    pub data: DataArgs,

    /// Address and port. `[::]` (the default) accepts IPv6 and IPv4 and falls
    /// back to `0.0.0.0` where IPv6 is unavailable; others bind as given.
    #[arg(long, env = "MUSICUS_LISTEN", default_value = "[::]:8080")]
    pub listen: SocketAddr,

    /// Directory of the built web client (output of `dx bundle`). Without
    /// it only the API is served.
    #[arg(long, env = "MUSICUS_WEB_DIR")]
    pub web_dir: Option<PathBuf>,

    /// Base URL printed in setup links, e.g. https://sheets.example.org.
    #[arg(long, env = "MUSICUS_PUBLIC_URL", default_value = "http://localhost:8080")]
    pub public_url: String,

    /// PEM certificate chain; together with --tls-key serves HTTPS directly.
    #[arg(long, env = "MUSICUS_TLS_CERT", requires = "tls_key")]
    pub tls_cert: Option<PathBuf>,

    #[arg(long, env = "MUSICUS_TLS_KEY", requires = "tls_cert")]
    pub tls_key: Option<PathBuf>,

    /// Send the session cookie without the `Secure` flag. Only for plain-HTTP
    /// development on another host than localhost; never in production.
    #[arg(long, env = "MUSICUS_INSECURE_COOKIES")]
    pub insecure_cookies: bool,

    /// Whether browsers may keep downloaded sheets in their HTTP cache.
    #[arg(long, env = "MUSICUS_CLIENT_CACHE", value_enum, default_value_t = ClientCache::None)]
    pub client_cache: ClientCache,

    /// RAM used to keep hot sheet files (e.g. tonight's programme), in MB.
    #[arg(long, env = "MUSICUS_RAM_CACHE_MB", default_value_t = 128)]
    pub ram_cache_mb: u64,

    /// Largest accepted upload, in MB.
    #[arg(long, env = "MUSICUS_MAX_UPLOAD_MB", default_value_t = 200)]
    pub max_upload_mb: u64,

    /// Days a session stays valid without use (sliding expiry).
    #[arg(long, env = "MUSICUS_SESSION_DAYS", default_value_t = 60)]
    pub session_days: u64,

    /// Take the client address from `X-Forwarded-For` (only behind a
    /// reverse proxy that sets it).
    #[arg(long, env = "MUSICUS_TRUST_PROXY")]
    pub trust_proxy: bool,

    /// Seconds the server keeps serving after the stop signal, so open
    /// clients can send what they have not saved yet (a-client-outage,
    /// R-OPS-043). Skipped when no client is connected; 0 to 30.
    #[arg(long, env = "MUSICUS_SHUTDOWN_GRACE_SECS", default_value_t = 3,
          value_parser = clap::value_parser!(u64).range(0..=MAX_SHUTDOWN_GRACE_SECS))]
    pub shutdown_grace_secs: u64,
}

/// Upper bound of `--shutdown-grace-secs`: with the drain limit it stays
/// inside the stop timeout of `compose.yaml` (`stop_grace_period`).
pub const MAX_SHUTDOWN_GRACE_SECS: u64 = 30;

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientCache {
    /// `Cache-Control: no-store`: clients keep nothing on disk.
    None,
    /// Files are immutable and may be cached privately for a year.
    Immutable,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(vars: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<OsString> {
        move |key| vars.iter().find(|(k, _)| *k == key).map(|(_, v)| OsString::from(v))
    }

    fn resolve(os: Os, vars: &'static [(&'static str, &'static str)]) -> Result<DataDir, String> {
        resolve_data_dir(None, os, &env(vars), false)
    }

    #[test]
    fn linux_uses_xdg_data_home_or_local_share() {
        let dir = resolve(Os::Linux, &[("HOME", "/home/a"), ("XDG_DATA_HOME", "/xdg")]).unwrap();
        assert_eq!(dir, DataDir { path: "/xdg/musicus".into(), source: DataDirSource::SystemDefault });
        let dir = resolve(Os::Linux, &[("HOME", "/home/a")]).unwrap();
        assert_eq!(dir.path, PathBuf::from("/home/a/.local/share/musicus"));
        // Empty or relative XDG_DATA_HOME is ignored (XDG base directory specification).
        let dir = resolve(Os::Linux, &[("HOME", "/home/a"), ("XDG_DATA_HOME", "")]).unwrap();
        assert_eq!(dir.path, PathBuf::from("/home/a/.local/share/musicus"));
        let dir = resolve(Os::Linux, &[("HOME", "/home/a"), ("XDG_DATA_HOME", "rel")]).unwrap();
        assert_eq!(dir.path, PathBuf::from("/home/a/.local/share/musicus"));
    }

    #[test]
    fn macos_uses_application_support() {
        let dir = resolve(Os::MacOs, &[("HOME", "/Users/a"), ("XDG_DATA_HOME", "/xdg")]).unwrap();
        assert_eq!(dir.path, PathBuf::from("/Users/a/Library/Application Support/Musicus"));
    }

    #[test]
    fn windows_uses_appdata() {
        let dir = resolve(Os::Windows, &[("APPDATA", "C:/Users/a/AppData/Roaming"), ("HOME", "/h")]).unwrap();
        assert_eq!(dir.path, PathBuf::from("C:/Users/a/AppData/Roaming").join("Musicus"));
    }

    #[test]
    fn missing_home_asks_for_a_folder() {
        assert!(resolve(Os::Linux, &[]).unwrap_err().contains("HOME"));
        assert!(resolve(Os::Windows, &[("HOME", "/h")]).unwrap_err().contains("APPDATA"));
    }

    #[test]
    fn configured_wins_then_existing_legacy_folder() {
        let vars = env(&[("HOME", "/home/a")]);
        let dir = resolve_data_dir(Some("/srv/m".into()), Os::Linux, &vars, true).unwrap();
        assert_eq!(dir, DataDir { path: "/srv/m".into(), source: DataDirSource::Configured });
        let dir = resolve_data_dir(None, Os::Linux, &vars, true).unwrap();
        assert_eq!(dir, DataDir { path: "data".into(), source: DataDirSource::Legacy });
        // A legacy database is used even when no system folder can be found.
        assert_eq!(resolve_data_dir(None, Os::Windows, &env(&[]), true).unwrap().source, DataDirSource::Legacy);
    }

    #[test]
    fn data_dir_flag_is_parsed() {
        let cli = Cli::try_parse_from(["musicus-server", "backup", "--data-dir", "/x", "--to", "b"]).unwrap();
        let Command::Backup { data, .. } = cli.command else { panic!() };
        assert_eq!(data.data_dir, Some(PathBuf::from("/x")));
    }

    use clap::CommandFactory;

    /// Per-call inputs of one-off commands, not settings of the program:
    /// they name what this one call works on, so an environment variable
    /// would only make a forgotten value apply to the next call as well.
    /// (`--help` and `--version` are clap's own and are skipped by id.)
    const NOT_SETTINGS: &[(&str, &str)] =
        &[("create-admin", "username"), ("create-admin", "display_name"), ("backup", "to"), ("restore", "from")];

    /// R-OPS-002: every option of every subcommand has a `MUSICUS_*` variable,
    /// so a new option without one fails here.
    #[test]
    fn every_setting_has_a_musicus_environment_variable() {
        let cli = Cli::command();
        let mut missing = Vec::new();
        let mut exempt_seen = Vec::new();
        let mut checked = Vec::new();
        for sub in cli.get_subcommands() {
            for arg in sub.get_arguments() {
                let id = arg.get_id().as_str();
                if matches!(id, "help" | "version") {
                    continue;
                }
                if NOT_SETTINGS.contains(&(sub.get_name(), id)) {
                    exempt_seen.push((sub.get_name(), id));
                    continue;
                }
                checked.push((sub.get_name(), id));
                match arg.get_env().and_then(|e| e.to_str()) {
                    Some(env) if env.starts_with("MUSICUS_") => {}
                    other => missing.push(format!("{} --{id}: {other:?}", sub.get_name())),
                }
            }
        }
        assert!(missing.is_empty(), "options without a MUSICUS_* variable: {missing:?}");
        assert_eq!(exempt_seen.len(), NOT_SETTINGS.len(), "stale NOT_SETTINGS entry: {exempt_seen:?}");
        assert!(cli.get_arguments().all(|a| matches!(a.get_id().as_str(), "help" | "version")));
        // The walk reaches the options of every command, including flattened ones.
        for expected in [
            ("serve", "listen"),
            ("serve", "data_dir"),
            ("serve", "web_dir"),
            ("serve", "trust_proxy"),
            ("create-admin", "public_url"),
            ("backup", "data_dir"),
            ("restore", "data_dir"),
            ("retry-update", "data_dir"),
            ("healthcheck", "listen"),
        ] {
            assert!(checked.contains(&expected), "{expected:?} not walked: {checked:?}");
        }
    }

    /// The child half of `listen_is_read_from_the_environment`: runs only in
    /// the child process that has the variable set (no `set_var` in this
    /// multi-threaded test process).
    #[test]
    #[ignore = "run by listen_is_read_from_the_environment"]
    fn listen_from_environment_child() {
        if std::env::var_os("MUSICUS_TEST_CHILD").is_none() {
            return; // `--include-ignored` in the normal run: nothing to check here
        }
        assert_eq!(std::env::var("MUSICUS_LISTEN").as_deref(), Ok("0.0.0.0:9000"));
        let Command::Serve(args) = Cli::try_parse_from(["musicus-server", "serve"]).unwrap().command else {
            unreachable!()
        };
        assert_eq!(args.listen, "0.0.0.0:9000".parse::<SocketAddr>().unwrap());
        assert_eq!(args.listen.port(), 9000);
        let Command::Healthcheck { listen } = Cli::try_parse_from(["musicus-server", "healthcheck"]).unwrap().command
        else {
            unreachable!()
        };
        assert_eq!(listen.port(), 9000);
        // An option given on the command line wins over the variable.
        let Command::Serve(args) =
            Cli::try_parse_from(["musicus-server", "serve", "--listen", "127.0.0.1:9100"]).unwrap().command
        else {
            unreachable!()
        };
        assert_eq!(args.listen.port(), 9100);
    }

    /// R-OPS-002 acceptance: `MUSICUS_LISTEN=0.0.0.0:9000` makes the server
    /// listen on port 9000. Parsed in a child process of this test binary
    /// with the variable set there only.
    #[test]
    fn listen_is_read_from_the_environment() {
        let mut child = std::process::Command::new(std::env::current_exe().unwrap());
        child.args(["config::tests::listen_from_environment_child", "--exact", "--ignored", "--test-threads=1"]);
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("MUSICUS_") {
                child.env_remove(key);
            }
        }
        let out = child.env("MUSICUS_LISTEN", "0.0.0.0:9000").env("MUSICUS_TEST_CHILD", "1").output().unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(out.status.success(), "{stdout}\n{}", String::from_utf8_lossy(&out.stderr));
        assert!(stdout.contains("1 passed"), "child test did not run: {stdout}");
    }

    /// Without the variable the default is the dual-stack `[::]:8080`.
    #[test]
    fn shutdown_grace_is_bounded() {
        let parse = |v: &str| Cli::try_parse_from(["musicus-server", "serve", "--shutdown-grace-secs", v]);
        let Command::Serve(args) = Cli::try_parse_from(["musicus-server", "serve"]).unwrap().command else {
            unreachable!()
        };
        assert_eq!(args.shutdown_grace_secs, 3);
        assert!(parse("0").is_ok() && parse("30").is_ok());
        assert!(parse("31").is_err() && parse("-1").is_err());
    }

    #[test]
    fn listen_default_is_dual_stack() {
        let cmd = Cli::command();
        let serve = cmd.find_subcommand("serve").unwrap();
        let listen = serve.get_arguments().find(|a| a.get_id() == "listen").unwrap();
        assert_eq!(listen.get_default_values(), ["[::]:8080"]);
    }
}
