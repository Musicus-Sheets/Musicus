//! Start, service and shutdown as open clients see them (F-OPS-008,
//! a-client-outage).
//!
//! `serve` listens before the database is opened: until [`Lifecycle::ready`]
//! every API request except health, version and the event stream is
//! answered with 503 `server_starting` (R-OPS-045). On the stop signal
//! [`Lifecycle::shut_down`] announces `shutting_down` on every event stream,
//! keeps serving for a short grace (open clients send what they have not
//! saved yet), then answers 503 `server_shutting_down` to new requests and
//! waits for the running ones (R-OPS-043). The caller then stops the
//! listener and closes the database ([`crate::db::Db::close`]).

use crate::{error::AppError, state::AppState};
use axum::Router;
use musicus_api::{ErrorCode, ServerPhase};
use std::{
    sync::{
        Arc, OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::{Notify, watch};

/// Upper bound for the requests still running when the grace is over.
pub const DRAIN_LIMIT: Duration = Duration::from_secs(5);

/// The phases of one server process, in this order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Listening; snapshot, updates and checks still run.
    Starting,
    Ready,
    /// Stop signal received: announced, requests still served (the grace).
    Draining,
    /// Grace over: new requests get `server_shutting_down`; the running
    /// ones finish, then the process exits.
    Closed,
}

impl Phase {
    /// The phase as clients see it.
    pub fn wire(self) -> ServerPhase {
        match self {
            Phase::Starting => ServerPhase::Starting,
            Phase::Ready => ServerPhase::Ready,
            Phase::Draining | Phase::Closed => ServerPhase::ShuttingDown,
        }
    }

    /// The answer to a request this phase does not serve.
    pub fn refusal(self) -> Option<AppError> {
        match self {
            Phase::Starting => Some(AppError::Unavailable(ErrorCode::ServerStarting)),
            Phase::Closed => Some(AppError::Unavailable(ErrorCode::ServerShuttingDown)),
            Phase::Ready | Phase::Draining => None,
        }
    }
}

pub struct Lifecycle {
    phase: watch::Sender<Phase>,
    app: OnceLock<AppState>,
    /// The API (paths below `/api`) with its state, set by [`Lifecycle::ready`].
    api: OnceLock<Router>,
    in_flight: AtomicUsize,
    idle: Notify,
    streams: AtomicUsize,
    /// Set when [`Lifecycle::shut_down`] is done: the listener may stop.
    stopped: watch::Sender<bool>,
}

impl Lifecycle {
    pub fn new() -> Arc<Self> {
        Arc::new(Lifecycle {
            phase: watch::Sender::new(Phase::Starting),
            app: OnceLock::new(),
            api: OnceLock::new(),
            in_flight: AtomicUsize::new(0),
            idle: Notify::new(),
            streams: AtomicUsize::new(0),
            stopped: watch::Sender::new(false),
        })
    }

    /// Ready from the start (tests, `routes::app`).
    pub fn serving(app: AppState) -> Arc<Self> {
        let lc = Self::new();
        lc.ready(app);
        lc
    }

    /// The start is done: serve the API with `app`. A stop signal that came
    /// during the start keeps its phase.
    pub fn ready(&self, app: AppState) {
        let api = crate::routes::api().with_state(app.clone());
        let _ = self.api.set(api);
        let _ = self.app.set(app);
        self.phase.send_if_modified(|p| {
            let starting = *p == Phase::Starting;
            if starting {
                *p = Phase::Ready;
            }
            starting
        });
    }

    pub fn phase(&self) -> Phase {
        *self.phase.borrow()
    }

    pub fn subscribe(&self) -> watch::Receiver<Phase> {
        self.phase.subscribe()
    }

    pub fn app(&self) -> Option<&AppState> {
        self.app.get()
    }

    pub(crate) fn api(&self) -> Option<&Router> {
        self.api.get()
    }

    /// Counts a request as running until the guard is dropped, or refuses it
    /// in a phase that serves none. Counting before looking at the phase
    /// means [`Lifecycle::shut_down`] never misses a request it let in.
    pub fn enter(self: &Arc<Self>) -> Result<InFlight, AppError> {
        self.in_flight.fetch_add(1, Ordering::SeqCst);
        let guard = InFlight(self.clone());
        match self.phase().refusal() {
            Some(e) => Err(e),
            None => Ok(guard),
        }
    }

    pub fn in_flight(&self) -> usize {
        self.in_flight.load(Ordering::SeqCst)
    }

    /// Open event streams.
    pub fn streams(&self) -> usize {
        self.streams.load(Ordering::SeqCst)
    }

    pub(crate) fn stream_guard(self: &Arc<Self>) -> StreamGuard {
        self.streams.fetch_add(1, Ordering::SeqCst);
        StreamGuard(self.clone())
    }

    /// The orderly stop (R-OPS-043): announce, serve for `grace` (skipped
    /// when no client listens), refuse new requests, wait at most `limit`
    /// for the running ones. Ends every event stream.
    pub async fn shut_down(&self, grace: Duration, limit: Duration) {
        self.phase.send_replace(Phase::Draining);
        if self.streams() > 0 && !grace.is_zero() {
            tracing::info!("shutting down: serving open clients for {} s", grace.as_secs_f32());
            tokio::time::sleep(grace).await;
        }
        self.phase.send_replace(Phase::Closed);
        let wait = async {
            loop {
                let idle = self.idle.notified();
                if self.in_flight() == 0 {
                    return;
                }
                idle.await;
            }
        };
        if tokio::time::timeout(limit, wait).await.is_err() {
            tracing::warn!("shutting down with {} requests still running", self.in_flight());
        }
        self.stopped.send_replace(true);
    }

    /// Resolves when [`Lifecycle::shut_down`] is done.
    pub async fn stopped(&self) {
        let mut rx = self.stopped.subscribe();
        let _ = rx.wait_for(|s| *s).await;
    }

    /// Stops at once (a start that failed): nothing to drain.
    pub fn abort(&self) {
        self.phase.send_replace(Phase::Closed);
        self.stopped.send_replace(true);
    }
}

/// A running request (see [`Lifecycle::enter`]).
pub struct InFlight(Arc<Lifecycle>);

impl Drop for InFlight {
    fn drop(&mut self) {
        if self.0.in_flight.fetch_sub(1, Ordering::SeqCst) == 1 {
            self.0.idle.notify_waiters();
        }
    }
}

/// An open event stream.
pub(crate) struct StreamGuard(Arc<Lifecycle>);

impl Drop for StreamGuard {
    fn drop(&mut self) {
        self.0.streams.fetch_sub(1, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phases_on_the_wire_and_refusals() {
        assert_eq!(Phase::Draining.wire(), ServerPhase::ShuttingDown);
        assert_eq!(Phase::Closed.wire(), ServerPhase::ShuttingDown);
        assert!(matches!(Phase::Starting.refusal(), Some(AppError::Unavailable(ErrorCode::ServerStarting))));
        assert!(matches!(Phase::Closed.refusal(), Some(AppError::Unavailable(ErrorCode::ServerShuttingDown))));
        assert!(Phase::Ready.refusal().is_none() && Phase::Draining.refusal().is_none());
    }

    #[tokio::test]
    async fn shut_down_waits_for_running_requests() {
        let lc = Lifecycle::new();
        lc.phase.send_replace(Phase::Ready);
        let Ok(running) = lc.enter() else { panic!("ready serves") };
        let stop = tokio::spawn({
            let lc = lc.clone();
            async move { lc.shut_down(Duration::from_secs(60), Duration::from_secs(10)).await }
        });
        // No stream open: no grace; closed at once, waiting for `running`.
        tokio::time::timeout(Duration::from_secs(2), async {
            while lc.phase() != Phase::Closed {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(matches!(lc.enter(), Err(AppError::Unavailable(ErrorCode::ServerShuttingDown))));
        assert!(!stop.is_finished());
        drop(running);
        tokio::time::timeout(Duration::from_secs(2), stop).await.unwrap().unwrap();
        assert_eq!(lc.in_flight(), 0);
        tokio::time::timeout(Duration::from_secs(1), lc.stopped()).await.unwrap();
    }
}
