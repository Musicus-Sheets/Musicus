//! `GET /api/events`: Server-Sent Events with the server's phase for open
//! clients (a-client-outage, F-OPS-008).
//!
//! One stream per tab. The first event is the current [`ServerState`], then
//! one per change (`starting` → `ready`, `ready` → `shutting_down`). Needs a
//! session once the server is ready; while it starts there is no database to
//! check one, so a stream opened then is accepted without (it carries only the
//! phase and the version, both public, a-version-public) and stays open. The
//! stream ends when the grace after the stop signal is over; browsers
//! reconnect by themselves (`retry`).

use crate::{
    auth::AuthUser,
    error::AppError,
    lifecycle::{Lifecycle, Phase},
};
use axum::{
    extract::{FromRequestParts, State},
    http::{HeaderValue, header, request::Parts},
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
};
use futures_util::stream;
use musicus_api::{STATE_EVENT, ServerPhase, ServerState};
use std::{convert::Infallible, sync::Arc, time::Duration};

/// Comment lines in between, so proxies keep an idle stream open.
pub const KEEP_ALIVE: Duration = Duration::from_secs(15);
/// How long a browser waits before it reconnects a broken stream.
pub const RETRY: Duration = Duration::from_secs(3);

pub async fn stream(State(lc): State<Arc<Lifecycle>>, mut parts: Parts) -> Result<Response, AppError> {
    match lc.phase() {
        Phase::Closed => return Err(AppError::Unavailable(musicus_api::ErrorCode::ServerShuttingDown)),
        Phase::Starting => {}
        Phase::Ready | Phase::Draining => {
            let app = lc.app().expect("ready has the state").clone();
            AuthUser::from_request_parts(&mut parts, &app).await?;
        }
    }
    let guard = lc.stream_guard();
    let rx = lc.subscribe();
    let events = stream::unfold((rx, None::<ServerPhase>, guard), |(mut rx, sent, guard)| async move {
        loop {
            let phase = *rx.borrow_and_update();
            let wire = phase.wire();
            if sent != Some(wire) {
                let mut event = Event::default()
                    .event(STATE_EVENT)
                    .json_data(ServerState::current(wire))
                    .expect("state serialises");
                if sent.is_none() {
                    event = event.retry(RETRY);
                }
                return Some((Ok::<_, Infallible>(event), (rx, Some(wire), guard)));
            }
            if phase == Phase::Closed || rx.changed().await.is_err() {
                return None;
            }
        }
    });
    let mut response = Sse::new(events).keep_alive(KeepAlive::new().interval(KEEP_ALIVE)).into_response();
    let h = response.headers_mut();
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    // nginx buffers proxied answers unless told otherwise.
    h.insert("x-accel-buffering", HeaderValue::from_static("no"));
    Ok(response)
}
