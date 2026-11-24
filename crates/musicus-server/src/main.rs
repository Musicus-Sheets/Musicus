use clap::Parser;
use musicus_server::{
    anyhow_like::{Result, plain},
    backup::{self, DataLock},
    bootstrap_admin,
    config::{Cli, Command, DataArgs, DataDirSource, ServeArgs},
    lifecycle::{DRAIN_LIMIT, Lifecycle},
    open_state, routes,
    state::{AppState, Settings},
    web::WebClient,
};
use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    time::Duration,
};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,tower_http=warn,sqlx=warn")),
        )
        .init();
    match Cli::parse().command {
        Command::Serve(args) => serve(args).await,
        Command::CreateAdmin { data, username, display_name, public_url } => {
            let data_dir = data_dir(&data)?;
            let _lock = DataLock::shared(&data_dir).map_err(plain)?;
            let app = open_state(&data_dir, 0, Settings::default()).await?;
            let display_name = display_name.unwrap_or_else(|| username.clone());
            let id = routes::users::insert_user(&app, &username, &display_name, musicus_api::Role::Admin).await?;
            let invite = routes::users::issue_invite(&app, id).await?;
            println!("Admin '{username}' created. Set the password here (valid 7 days):");
            println!("{}/invite/{}", public_url.trim_end_matches('/'), invite.token);
            Ok(())
        }
        Command::Backup { data, to } => {
            let data_dir = data_dir(&data)?;
            let done = backup::backup(&data_dir, &to).await.map_err(plain)?;
            match done.version {
                Some(v) => println!("Database (last used by Musicus {v}) written to {}.", to.display()),
                None => println!("Database written to {}.", to.display()),
            }
            println!("Now back up {} (immutable files; copy after the database).", data_dir.join("blobs").display());
            println!("Keep backups as safe as the server: they contain all sheets, markings and password hashes.");
            Ok(())
        }
        Command::Restore { data, from } => {
            let data_dir = data_dir(&data)?;
            let done = backup::restore(&data_dir, &from, musicus_server::db::now()).await.map_err(plain)?;
            let by = done.version.map(|v| format!(" (last used by Musicus {v})")).unwrap_or_default();
            println!("Database restored from {}{by}.", from.display());
            if let Some(old) = &done.replaced {
                println!("The replaced database is kept as {}.", old.display());
            }
            println!("Nothing in {} was changed.", data_dir.join("blobs").display());
            if done.missing_files > 0 {
                println!(
                    "Warning: {} sheet files of the restored database are missing in blobs/; copy the blobs \
                     folder of the same backup back (files that exist are never overwritten).",
                    done.missing_files
                );
            }
            println!(
                "The next start of Musicus brings the database to its own version. A release older than the one \
                 that last used it starts only within the downgrade window; otherwise it stops with a message \
                 naming the release to start."
            );
            if let Some(note) = &done.guard_note {
                println!(
                    "Note: {} is still there, so the release whose update failed will not try again; \
                     run `musicus-server retry-update` when it should.",
                    note.display()
                );
            }
            Ok(())
        }
        Command::RetryUpdate { data } => {
            let data_dir = data_dir(&data)?;
            if backup::retry_update(&data_dir).map_err(plain)? {
                println!("The next start will try the failed database update again.");
            } else {
                println!("No failed database update was noted in {}.", data_dir.display());
            }
            Ok(())
        }
        Command::Healthcheck { listen } => healthcheck(listen),
    }
}

/// Resolves the data directory and says which one is used (R-OPS-031).
fn data_dir(args: &DataArgs) -> Result<PathBuf> {
    let dir = args.resolve()?;
    let shown = std::path::absolute(&dir.path).unwrap_or_else(|_| dir.path.clone());
    match dir.source {
        DataDirSource::Configured => tracing::info!("data folder: {}", shown.display()),
        DataDirSource::SystemDefault => tracing::info!(
            "data folder: {} (the usual folder of this system; choose another with --data-dir)",
            shown.display()
        ),
        DataDirSource::Legacy => {
            let usual = musicus_server::config::default_data_dir(musicus_server::config::Os::current(), &|key| {
                std::env::var_os(key)
            })
            .map_or_else(|| "the usual folder of this system".to_owned(), |d| d.display().to_string());
            tracing::warn!(
                "data folder: {} (found a database there). Without --data-dir, Musicus now uses {usual}; \
                 start it with --data-dir {} to keep this folder, or move the folder's contents there",
                shown.display(),
                dir.path.display()
            );
        }
    }
    Ok(dir.path)
}

/// A minimal HTTP/1.0 request, so the runtime image needs no curl or shell.
fn healthcheck(listen: SocketAddr) -> Result<()> {
    use std::io::{Read, Write};
    use std::time::Duration;
    // `[::]` → `[::1]`, then `127.0.0.1` (the server may have fallen back to IPv4).
    let mut stream = None;
    let mut failure = None;
    for target in musicus_server::listen::local_targets(listen) {
        match std::net::TcpStream::connect_timeout(&target, Duration::from_secs(3)) {
            Ok(s) => {
                stream = Some(s);
                break;
            }
            Err(e) => failure = Some(e),
        }
    }
    let Some(mut stream) = stream else { return Err(failure.expect("at least one target").into()) };
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    stream.write_all(b"GET /api/health HTTP/1.0\r\nHost: localhost\r\n\r\n")?;
    let mut head = [0u8; 12];
    stream.read_exact(&mut head)?;
    if head.ends_with(b" 200") { Ok(()) } else { Err(format!("unhealthy: {}", String::from_utf8_lossy(&head)).into()) }
}

/// Start-up sequence (a-client-outage, R-OPS-045): listen first and answer
/// `server_starting`, then open the data (snapshot, updates, checks; a failed
/// update still ends the process with an error after the restore), then
/// serve. On the stop signal: the orderly drain of `Lifecycle::shut_down`,
/// then the listener stops and the database is closed (R-OPS-043).
async fn serve(args: ServeArgs) -> Result<()> {
    let settings = Settings {
        cookie_secure: !args.insecure_cookies,
        client_cache: args.client_cache,
        session_ttl_secs: args.session_days as i64 * 24 * 3600,
        max_upload_bytes: args.max_upload_mb * 1024 * 1024,
        trust_proxy: args.trust_proxy,
        ..Settings::default()
    };
    let data_dir = data_dir(&args.data)?;
    // Held while the server runs: `restore` refuses meanwhile (R-OPS-022).
    let _lock = DataLock::shared(&data_dir).map_err(plain)?;
    let web = WebClient::choose(args.web_dir.clone());
    match &web {
        WebClient::Dir(dir) if !dir.join("index.html").exists() => {
            return Err(format!("{} contains no index.html (build the web client first)", dir.display()).into());
        }
        WebClient::Dir(dir) => tracing::info!("web client: folder {}", dir.display()),
        WebClient::Builtin(assets) => tracing::info!("web client: built into the program ({} files)", assets.len()),
        WebClient::None => tracing::warn!(
            "this program contains no web client (built without the feature embed-web): only /api is served; \
             pass --web-dir with the folder of a built client"
        ),
    }

    let lc = Lifecycle::new();
    let app = routes::service(lc.clone(), web).into_make_service_with_connect_info::<SocketAddr>();
    let (listener, stack) = musicus_server::listen::bind(args.listen)?;
    let local = listener.local_addr()?;
    let mut server = match (&args.tls_cert, &args.tls_key) {
        (Some(cert), Some(key)) => {
            let _ = rustls::crypto::ring::default_provider().install_default();
            let config = axum_server::tls_rustls::RustlsConfig::from_pem_file(cert, key).await?;
            let handle = axum_server::Handle::new();
            let stop = handle.clone();
            let stopped = lc.clone();
            tokio::spawn(async move {
                stopped.stopped().await;
                stop.graceful_shutdown(Some(DRAIN_LIMIT));
            });
            tracing::info!("listening on https://{local} ({})", stack.describe());
            let server = axum_server::from_tcp_rustls(listener, config)?.handle(handle);
            tokio::spawn(async move { server.serve(app).await.map_err(|e| e.to_string()) })
        }
        _ => {
            let listener = tokio::net::TcpListener::from_std(listener)?;
            tracing::info!("listening on http://{local} ({})", stack.describe());
            let stopped = lc.clone();
            tokio::spawn(async move {
                axum::serve(listener, app)
                    .with_graceful_shutdown(async move { stopped.stopped().await })
                    .await
                    .map_err(|e| e.to_string())
            })
        }
    };

    // The stop signal may come at any time; a running start is not cut short.
    let grace = Duration::from_secs(args.shutdown_grace_secs);
    let mut signal = {
        let lc = lc.clone();
        tokio::spawn(async move {
            shutdown_signal().await;
            lc.shut_down(grace, DRAIN_LIMIT).await;
        })
    };

    let state = match start(&data_dir, &args, settings).await {
        Ok(state) => state,
        Err(e) => {
            lc.abort();
            signal.abort();
            let _ = tokio::time::timeout(DRAIN_LIMIT, &mut server).await;
            return Err(e);
        }
    };
    // Detects the music systems of new (and not yet analysed) files in the background.
    let worker = musicus_server::layout::worker::spawn(state.clone());
    lc.ready(state.clone());
    tracing::info!("ready");

    tokio::select! {
        _ = lc.stopped() => {}
        // The stop task ended without finishing the stop, e.g. a panic of
        // the log writer (stdout/stderr closed): stop anyway.
        _ = &mut signal => lc.abort(),
    }
    match tokio::time::timeout(DRAIN_LIMIT, &mut server).await {
        Ok(Ok(Err(e))) => tracing::warn!("server: {e}"),
        Ok(_) => {}
        Err(_) => {
            tracing::warn!("connections still open; closing them");
            server.abort();
        }
    }
    worker.abort();
    state.db.close().await;
    tracing::info!("stopped");
    Ok(())
}

/// Opens the data directory (snapshot, updates, checks) and creates the
/// first admin if there is none.
async fn start(data_dir: &Path, args: &ServeArgs, settings: Settings) -> Result<AppState> {
    let state = open_state(data_dir, args.ram_cache_mb * 1024 * 1024, settings).await?;
    if let Some(token) = bootstrap_admin(&state).await? {
        tracing::warn!("No users yet. Created 'admin'; set its password at:");
        tracing::warn!("{}/invite/{token}", args.public_url.trim_end_matches('/'));
    }
    Ok(state)
}

async fn shutdown_signal() {
    let ctrl_c = tokio::signal::ctrl_c();
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("stop signal received: shutting down");
}
