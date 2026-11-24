//! HTTP access to the Musicus REST API.
//!
//! The session lives in an HttpOnly cookie the browser sends by itself;
//! every request also carries the CSRF header the server requires for
//! cookie-authenticated writes.

//! Every answer (or its absence) is reported to [`connection`]; while the
//! server has announced that it is away, requests are refused here without
//! being sent (a-client-outage), except the annotation saves of the reader,
//! which flush what is pending ([`get_page`], [`put_page`]).

use crate::{
    bridge,
    connection::{self, Input},
    errors,
};
use dioxus::prelude::ReadableExt;
use gloo_net::http::{Method, Request, RequestBuilder, Response};
use musicus_api::{ApiError, CSRF_HEADER, ErrorCode, ServerPhase};
use serde::{Serialize, de::DeserializeOwned};
use std::cell::Cell;

#[derive(Debug, Clone, PartialEq)]
pub struct ApiErr {
    /// HTTP status, 0 for network errors.
    pub status: u16,
    /// The server's code; `None` for network errors and unreadable bodies.
    pub code: Option<ErrorCode>,
    /// Text for the user, from [`errors`] (the server sends no prose).
    pub message: String,
}

impl std::fmt::Display for ApiErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

pub type ApiResult<T> = Result<T, ApiErr>;

fn network(e: impl std::fmt::Display) -> ApiErr {
    ApiErr { status: 0, code: None, message: errors::network_text(&e.to_string()) }
}

/// The request got no answer: the server is not reachable (or going).
fn lost(e: gloo_net::Error) -> ApiErr {
    connection::report(Input::Lost);
    network(e)
}

thread_local! {
    static RUNNING: Cell<usize> = const { Cell::new(0) };
}

/// Counts a request (or an upload of the bridge) as running while it lives;
/// the reload after an update waits for none to run.
pub struct Running(());

pub fn start_running() -> Running {
    RUNNING.with(|r| r.set(r.get() + 1));
    Running(())
}

impl Drop for Running {
    fn drop(&mut self) {
        RUNNING.with(|r| r.set(r.get().saturating_sub(1)));
    }
}

/// Requests running now.
pub fn running() -> usize {
    RUNNING.with(Cell::get)
}

/// Refuses a request while the server announced that it is away: nothing
/// new may start (R-OPS-044), the banner says why.
fn gate() -> ApiResult<()> {
    let code = match connection::CONN.peek().server {
        connection::Server::ShuttingDown => ErrorCode::ServerShuttingDown,
        connection::Server::Starting => ErrorCode::ServerStarting,
        _ => return Ok(()),
    };
    Err(ApiErr { status: 503, message: errors::code_text(&code), code: Some(code) })
}

fn builder(method: Method, path: &str) -> RequestBuilder {
    RequestBuilder::new(path).method(method).header(CSRF_HEADER, "1")
}

/// Sends a request (no gate), counted as running, reporting a missing answer.
async fn fetch(request: Request) -> ApiResult<Response> {
    let _running = start_running();
    let write = !matches!(request.method(), Method::GET | Method::HEAD);
    let r = request.send().await.map_err(lost)?;
    if write && r.ok() {
        // What was typed has been saved (the reload rule of R-OPS-048).
        bridge::clear_unsaved_input();
    }
    Ok(r)
}

async fn check(response: Response) -> ApiResult<Response> {
    let status = response.status();
    if response.ok() {
        connection::report(Input::Answered);
        return Ok(response);
    }
    let body = response.text().await.unwrap_or_default();
    let parsed = serde_json::from_str::<ApiError>(&body);
    match parsed.as_ref().map(|e| &e.code) {
        Ok(ErrorCode::ServerStarting) => connection::report(Input::Refused(ServerPhase::Starting)),
        Ok(ErrorCode::ServerShuttingDown) => connection::report(Input::Refused(ServerPhase::ShuttingDown)),
        // A proxy answering for a server that is away (no Musicus error body).
        Err(_) if matches!(status, 502..=504) => connection::report(Input::Lost),
        _ => connection::report(Input::Answered),
    }
    let (code, message) = match parsed {
        Ok(e) => (Some(e.code.clone()), errors::api_error_text(&e)),
        // A code this build does not know, with arguments: the English message.
        Err(_) => match serde_json::from_str::<serde_json::Value>(&body) {
            Ok(v) if v["message"].is_string() => {
                (None, errors::unknown_error_text(v["message"].as_str().unwrap_or("")))
            }
            _ => (None, errors::http_failed_text(status)),
        },
    };
    if status == 401 {
        session_lost();
    }
    Err(ApiErr { status, code, message })
}

/// The session expired or was revoked: back to the login page (a full
/// reload clears all in-memory state).
fn session_lost() {
    if let Some(window) = web_sys::window() {
        let path = window.location().pathname().unwrap_or_default();
        if path != "/login" && !path.starts_with("/invite/") {
            let _ = window.location().set_href("/login");
        }
    }
}

/// `GET /api/version`: the release the server runs (public, R-OPS-033).
/// Differs from [`musicus_api::VERSION`] when the server was updated after
/// this client was loaded.
pub async fn version() -> ApiResult<String> {
    let r = builder(Method::GET, "/api/version").send().await.map_err(network)?;
    if !r.ok() {
        return Err(ApiErr { status: r.status(), code: None, message: errors::http_failed_text(r.status()) });
    }
    r.json::<musicus_api::VersionInfo>().await.map(|v| v.version).map_err(network)
}

/// `GET /api/health` for the polling while the server is away (not gated,
/// reports nothing itself): `None` = ready, `Some(phase)` = starting or
/// shutting down, `Err` = no answer from Musicus.
pub async fn health() -> Result<Option<ServerPhase>, ()> {
    let r = builder(Method::GET, "/api/health").send().await.map_err(|_| ())?;
    if r.ok() {
        return Ok(None);
    }
    match r.json::<ApiError>().await.map(|e| e.code) {
        Ok(ErrorCode::ServerStarting) => Ok(Some(ServerPhase::Starting)),
        Ok(ErrorCode::ServerShuttingDown) => Ok(Some(ServerPhase::ShuttingDown)),
        _ => Err(()),
    }
}

pub async fn get<T: DeserializeOwned>(path: &str) -> ApiResult<T> {
    gate()?;
    let r = check(fetch(builder(Method::GET, path).build().map_err(network)?).await?).await?;
    r.json().await.map_err(network)
}

pub async fn send<B: Serialize, T: DeserializeOwned>(method: Method, path: &str, body: &B) -> ApiResult<T> {
    gate()?;
    let request: Request = builder(method, path).json(body).map_err(network)?;
    let r = check(fetch(request).await?).await?;
    r.json().await.map_err(network)
}

pub async fn post<B: Serialize, T: DeserializeOwned>(path: &str, body: &B) -> ApiResult<T> {
    send(Method::POST, path, body).await
}

pub async fn put<B: Serialize, T: DeserializeOwned>(path: &str, body: &B) -> ApiResult<T> {
    send(Method::PUT, path, body).await
}

pub async fn patch<B: Serialize, T: DeserializeOwned>(path: &str, body: &B) -> ApiResult<T> {
    send(Method::PATCH, path, body).await
}

/// A request without a JSON answer (204).
pub async fn call(method: Method, path: &str) -> ApiResult<()> {
    gate()?;
    check(fetch(builder(method, path).build().map_err(network)?).await?).await.map(|_| ())
}

/// A JSON request without a JSON answer (204).
pub async fn call_json<B: Serialize>(method: Method, path: &str, body: &B) -> ApiResult<()> {
    gate()?;
    let request: Request = builder(method, path).json(body).map_err(network)?;
    check(fetch(request).await?).await.map(|_| ())
}

/// Uploads raw bytes (a sheet file).
pub async fn upload<T: DeserializeOwned>(path: &str, bytes: &[u8], content_type: &str) -> ApiResult<T> {
    gate()?;
    let body = js_sys::Uint8Array::from(bytes);
    let request = builder(Method::PUT, path).header("content-type", content_type).body(body).map_err(network)?;
    let r = check(fetch(request).await?).await?;
    r.json().await.map_err(network)
}

/// Outcome of loading an annotation page.
pub enum PageBytes {
    Found { bytes: Vec<u8>, revision: i64 },
    Missing,
}

/// Not gated: also part of a flush (merge after a conflict); the reader
/// does not load new pages while the server is announced away.
pub async fn get_page(set_id: i64, page: i64) -> ApiResult<PageBytes> {
    let request =
        builder(Method::GET, &format!("/api/annotation-sets/{set_id}/pages/{page}")).build().map_err(network)?;
    let r = fetch(request).await?;
    if r.status() == 204 {
        connection::report(Input::Answered);
        return Ok(PageBytes::Missing);
    }
    let r = check(r).await?;
    let revision = r.headers().get("etag").and_then(|e| e.trim_matches('"').parse().ok()).unwrap_or(0);
    Ok(PageBytes::Found { bytes: r.binary().await.map_err(network)?, revision })
}

/// Outcome of saving an annotation page: the new revision, or a conflict
/// with a save from another device.
pub enum SaveOutcome {
    Saved(i64),
    Conflict,
}

/// Not gated: the flush of pending pages runs while the server shuts down.
pub async fn put_page(set_id: i64, page: i64, expected_revision: i64, bytes: &[u8]) -> ApiResult<SaveOutcome> {
    let body = js_sys::Uint8Array::from(bytes);
    let request = builder(Method::PUT, &format!("/api/annotation-sets/{set_id}/pages/{page}"))
        .header("content-type", "application/octet-stream")
        .header("if-match", &format!("\"{expected_revision}\""))
        .body(body)
        .map_err(network)?;
    let r = fetch(request).await?;
    if r.status() == 409 {
        connection::report(Input::Answered);
        return Ok(SaveOutcome::Conflict);
    }
    let saved: musicus_api::SavedPage = check(r).await?.json().await.map_err(network)?;
    Ok(SaveOutcome::Saved(saved.revision))
}

pub fn file_url(sha256: &str) -> String {
    format!("/api/files/{sha256}")
}

pub fn thumbnail_url(sha256: &str) -> String {
    format!("/api/files/{sha256}/thumbnail")
}
