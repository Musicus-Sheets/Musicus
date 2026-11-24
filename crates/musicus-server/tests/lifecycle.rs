//! Open clients during start and shutdown (F-OPS-008, a-client-outage):
//! the starting gate, the event stream and the drain, driven in-process
//! through `routes::service`. The real process (signal, exit, closed
//! database) is tested in `tests/shutdown.rs`.

use axum::{
    Router,
    body::{Body, Bytes},
    http::{Method, Request, StatusCode, header},
    response::Response,
};
use http_body_util::BodyExt;
use musicus_api::{ApiError, ErrorCode, Id, LoginResponse, ServerPhase, ServerState};
use musicus_server::{
    bootstrap_admin,
    lifecycle::{Lifecycle, Phase},
    open_state, routes,
    state::{AppState, Settings},
    web::WebClient,
};
use serde_json::json;
use std::{sync::Arc, time::Duration};
use tower::ServiceExt;

async fn send(router: &Router, method: Method, uri: &str, token: Option<&str>, body: Body) -> Response {
    let mut req = Request::builder().method(method).uri(uri).header("content-type", "application/json");
    if let Some(t) = token {
        req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    router.clone().oneshot(req.body(body).unwrap()).await.unwrap()
}

async fn body(r: Response) -> Vec<u8> {
    r.into_body().collect().await.unwrap().to_bytes().to_vec()
}

/// 503 with the code and `Retry-After`.
async fn unavailable(r: Response) -> ErrorCode {
    assert_eq!(r.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(r.headers()[header::RETRY_AFTER], "5");
    serde_json::from_slice::<ApiError>(&body(r).await).unwrap().code
}

/// Reads the event stream until the next `state` event.
async fn next_state(stream: &mut Body) -> Option<ServerState> {
    let mut text = String::new();
    loop {
        let frame = tokio::time::timeout(Duration::from_secs(5), stream.frame()).await.expect("event in time")?;
        let Ok(data) = frame.unwrap().into_data() else { continue };
        text.push_str(std::str::from_utf8(&data).unwrap());
        if let Some(end) = text.find("\n\n") {
            let event: String = text.drain(..end + 2).collect();
            if !event.contains("event: state") {
                continue; // keep-alive comment
            }
            let data = event.lines().find_map(|l| l.strip_prefix("data: ")).expect("data line");
            return Some(serde_json::from_str(data).unwrap());
        }
    }
}

async fn open_events(router: &Router, token: Option<&str>) -> Body {
    let r = send(router, Method::GET, "/api/events", token, Body::empty()).await;
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(r.headers()[header::CONTENT_TYPE], "text/event-stream");
    assert_eq!(r.headers()[header::CACHE_CONTROL], "no-cache");
    assert_eq!(r.headers()["x-accel-buffering"], "no");
    r.into_body()
}

async fn state(dir: &std::path::Path) -> (AppState, String) {
    let settings = Settings { cookie_secure: false, ..Settings::default() };
    let state = open_state(dir, 1024 * 1024, settings).await.unwrap();
    let invite = bootstrap_admin(&state).await.unwrap().unwrap();
    (state, invite)
}

/// The admin's bearer token.
async fn admin(router: &Router, invite: &str) -> String {
    let r = send(
        router,
        Method::POST,
        &format!("/api/auth/invites/{invite}"),
        None,
        Body::from(json!({"password": "admin-password-1"}).to_string()),
    )
    .await;
    assert_eq!(r.status(), StatusCode::OK);
    let login = json!({"username": "admin", "password": "admin-password-1", "bearer": true}).to_string();
    let r = send(router, Method::POST, "/api/auth/login", None, Body::from(login)).await;
    serde_json::from_slice::<LoginResponse>(&body(r).await).unwrap().token.unwrap()
}

#[tokio::test]
async fn starting_answers_server_starting_until_ready() {
    let dir = tempfile::tempdir().unwrap();
    let lc = Lifecycle::new();
    let router = routes::service(lc.clone(), WebClient::None);

    // Before the database is opened: only health, version and events answer.
    assert_eq!(unavailable(send(&router, Method::GET, "/api/health", None, Body::empty()).await).await, {
        ErrorCode::ServerStarting
    });
    for (method, uri) in [(Method::GET, "/api/scores"), (Method::POST, "/api/auth/login"), (Method::GET, "/api/nope")] {
        let r = send(&router, method, uri, None, Body::from("{}")).await;
        assert_eq!(unavailable(r).await, ErrorCode::ServerStarting, "{uri}");
    }
    let r = send(&router, Method::GET, "/api/version", None, Body::empty()).await;
    assert_eq!(r.status(), StatusCode::OK);
    // No session needed while starting (nothing to check it against).
    let mut events = open_events(&router, None).await;
    let first = next_state(&mut events).await.unwrap();
    assert_eq!(first, ServerState::current(ServerPhase::Starting));

    let (app, _) = state(dir.path()).await;
    lc.ready(app);
    assert_eq!(next_state(&mut events).await.unwrap(), ServerState::current(ServerPhase::Ready));
    let r = send(&router, Method::GET, "/api/health", None, Body::empty()).await;
    assert_eq!((r.status(), body(r).await), (StatusCode::OK, b"ok".to_vec()));
    let r = send(&router, Method::GET, "/api/scores", None, Body::empty()).await;
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    // Unknown API paths are a JSON 404 again.
    let r = send(&router, Method::GET, "/api/nope", None, Body::empty()).await;
    assert_eq!(r.status(), StatusCode::NOT_FOUND);
    // Once ready, a new stream needs a session.
    let r = send(&router, Method::GET, "/api/events", None, Body::empty()).await;
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
}

/// Creates a piece with one part; returns the part id.
async fn part(router: &Router, token: &str) -> Id {
    let r = send(router, Method::POST, "/api/scores", Some(token), Body::from(json!({"title": "March"}).to_string()));
    let score: musicus_api::Score = serde_json::from_slice(&body(r.await).await).unwrap();
    let uri = format!("/api/scores/{}/parts", score.id);
    let parts = Body::from(json!({"label": "Solo", "instrument_ids": []}).to_string());
    let r = send(router, Method::POST, &uri, Some(token), parts).await;
    let status = r.status();
    let bytes = body(r).await;
    assert!(status.is_success(), "{status} {}", String::from_utf8_lossy(&bytes));
    serde_json::from_slice::<musicus_api::Part>(&bytes).unwrap().id
}

fn pdf(fill: usize) -> Vec<u8> {
    let mut v = b"%PDF-1.4\n%".to_vec();
    v.extend(std::iter::repeat_n(b'x', fill));
    v
}

#[tokio::test]
async fn shutdown_announces_serves_the_grace_then_refuses_and_waits_for_running_requests() {
    let dir = tempfile::tempdir().unwrap();
    let (app, invite) = state(dir.path()).await;
    let lc = Lifecycle::serving(app.clone());
    let router = routes::service(lc.clone(), WebClient::None);
    let token = admin(&router, &invite).await;
    let part = part(&router, &token).await;
    let mut events = open_events(&router, Some(&token)).await;
    assert_eq!(next_state(&mut events).await.unwrap().state, ServerPhase::Ready);
    assert_eq!(lc.streams(), 1);

    // An upload that is still sending its body when the grace ends.
    let (tx, rx) = tokio::sync::mpsc::channel::<Bytes>(4);
    let chunks =
        futures_util::stream::unfold(
            rx,
            |mut rx| async move { rx.recv().await.map(|b| (Ok::<_, std::io::Error>(b), rx)) },
        );
    let upload = tokio::spawn({
        let router = router.clone();
        let token = token.clone();
        async move {
            send(&router, Method::PUT, &format!("/api/parts/{part}/file"), Some(&token), Body::from_stream(chunks))
                .await
        }
    });
    let file = pdf(4000);
    tx.send(Bytes::copy_from_slice(&file[..2000])).await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while lc.in_flight() == 0 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();

    let grace = Duration::from_millis(400);
    let stop = tokio::spawn({
        let lc = lc.clone();
        async move { lc.shut_down(grace, Duration::from_secs(10)).await }
    });
    // Announced at once, on the open stream.
    assert_eq!(next_state(&mut events).await.unwrap(), ServerState::current(ServerPhase::ShuttingDown));
    // During the grace everything is still served (the clients' last saves).
    let r = send(&router, Method::GET, "/api/auth/me", Some(&token), Body::empty()).await;
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(unavailable(send(&router, Method::GET, "/api/health", None, Body::empty()).await).await, {
        ErrorCode::ServerShuttingDown
    });

    tokio::time::sleep(grace + Duration::from_millis(200)).await;
    assert_eq!(lc.phase(), Phase::Closed);
    // New work after the grace: 503 `server_shutting_down`; the stream has ended.
    let r = send(&router, Method::GET, "/api/scores", Some(&token), Body::empty()).await;
    assert_eq!(unavailable(r).await, ErrorCode::ServerShuttingDown);
    let r = send(&router, Method::GET, "/api/events", Some(&token), Body::empty()).await;
    assert_eq!(unavailable(r).await, ErrorCode::ServerShuttingDown);
    assert!(next_state(&mut events).await.is_none(), "stream ends after the grace");
    assert_eq!(send(&router, Method::GET, "/api/version", None, Body::empty()).await.status(), StatusCode::OK);
    // The upload that was running is waited for and stored.
    assert!(!stop.is_finished(), "waits for the running upload");
    tx.send(Bytes::copy_from_slice(&file[2000..])).await.unwrap();
    drop(tx);
    let r = upload.await.unwrap();
    let status = r.status();
    let bytes = body(r).await;
    assert!(status.is_success(), "{status} {}", String::from_utf8_lossy(&bytes));
    tokio::time::timeout(Duration::from_secs(2), stop).await.unwrap().unwrap();
    tokio::time::timeout(Duration::from_secs(1), lc.stopped()).await.unwrap();
    let versions: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM part_files WHERE part_id = ?1")
        .bind(part)
        .fetch_one(&app.db.read)
        .await
        .unwrap();
    assert_eq!(versions, 1);
    app.db.close().await;
    let wal = dir.path().join("musicus.sqlite-wal");
    assert!(std::fs::metadata(&wal).map_or(true, |m| m.len() == 0), "WAL checkpointed");
}

#[tokio::test]
async fn without_open_streams_the_grace_is_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let (app, _) = state(dir.path()).await;
    let lc: Arc<Lifecycle> = Lifecycle::serving(app);
    let started = std::time::Instant::now();
    lc.shut_down(Duration::from_secs(30), Duration::from_secs(5)).await;
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(lc.phase(), Phase::Closed);
}

#[tokio::test]
async fn a_stop_signal_during_the_start_keeps_shutting_down() {
    let dir = tempfile::tempdir().unwrap();
    let lc = Lifecycle::new();
    let router = routes::service(lc.clone(), WebClient::None);
    let mut events = open_events(&router, None).await;
    assert_eq!(next_state(&mut events).await.unwrap().state, ServerPhase::Starting);
    let stop = tokio::spawn({
        let lc = lc.clone();
        async move { lc.shut_down(Duration::from_millis(100), Duration::from_secs(5)).await }
    });
    assert_eq!(next_state(&mut events).await.unwrap().state, ServerPhase::ShuttingDown);
    let (app, _) = state(dir.path()).await;
    lc.ready(app);
    stop.await.unwrap();
    assert_eq!(lc.phase(), Phase::Closed, "ready does not undo the stop");
    assert!(next_state(&mut events).await.is_none());
}
