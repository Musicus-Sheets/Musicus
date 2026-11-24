//! What the client knows about the server: ready, starting, shutting down or
//! not reachable, and whether it runs a newer version (F-OPS-008,
//! a-client-outage). The state machine ([`Conn`]) and the reload rule
//! ([`may_reload`]) are plain functions, tested on the host; the rest feeds
//! them from the event stream (`GET /api/events`, `bridge::open_events`), from
//! the answers of every request (`api.rs`) and from polling `/api/health`
//! while the server is away.
//!
//! Rules:
//! - Announced (`shutting_down` event or answer, `server_starting` answer):
//!   every action that needs the server is blocked, drawing included; what
//!   is pending is sent at once ([`Then::Flush`]: block first, then
//!   flush). Reading what is loaded keeps working.
//! - Not reachable without an announcement (network loss): requests are
//!   still tried, drawing goes on; drawings stay in the tab's memory.
//! - Back to ready: what is still pending is sent again ([`Then::Resend`]);
//!   a different version means "Musicus has been updated" and a reload at a
//!   safe moment ([`may_reload`]).

use musicus_api::{ServerPhase, ServerState};

/// The server as this tab sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Server {
    #[default]
    Ready,
    /// Announced by the server: snapshot, updates and checks run.
    Starting,
    /// Announced by the server: it stops.
    ShuttingDown,
    /// No answer, without an announcement (network loss, proxy without server).
    Unreachable,
}

/// What happened (input of [`Conn::apply`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Input {
    /// An event of the stream, or the result of polling health and version.
    State(ServerState),
    /// A request was answered by Musicus (any status but the two below).
    Answered,
    /// A 503 answer with `server_starting` / `server_shutting_down`.
    Refused(ServerPhase),
    /// No answer: network error, or a proxy reporting that Musicus is away.
    Lost,
}

/// What the caller must do after [`Conn::apply`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Then {
    None,
    /// The server announced its stop: send everything not sent yet, now
    /// (new actions are already blocked).
    Flush,
    /// The server is back: send again what is still pending.
    Resend,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Conn {
    pub server: Server,
    /// The version the server runs when it differs from this client's.
    pub update: Option<String>,
}

impl Conn {
    /// Applies `input`; `own` is this client's version (`musicus_api::VERSION`).
    pub fn apply(&mut self, input: Input, own: &str) -> Then {
        let before = self.server;
        let after = match input {
            Input::State(s) => {
                if s.state == ServerPhase::Ready {
                    self.update = (s.version != own).then_some(s.version);
                }
                phase(s.state)
            }
            Input::Refused(p) => phase(p),
            // Answers during the stop (the flush) do not mean the server is back.
            Input::Answered if before == Server::Unreachable => Server::Ready,
            Input::Answered => before,
            Input::Lost => match before {
                // A stopping server going away is expected; one that does not
                // finish its start (a failed update) is not reachable.
                Server::ShuttingDown => Server::ShuttingDown,
                _ => Server::Unreachable,
            },
        };
        self.server = after;
        match (before, after) {
            (b, Server::ShuttingDown) if b != Server::ShuttingDown => Then::Flush,
            (b, Server::Ready) if b != Server::Ready => Then::Resend,
            _ => Then::None,
        }
    }

    /// The server announced that it is away: nothing that needs it may start,
    /// drawing included.
    pub fn announced(&self) -> bool {
        matches!(self.server, Server::Starting | Server::ShuttingDown)
    }

    /// Drawing is allowed unless the server announced that it is away (when
    /// it is merely not reachable, drawings wait in memory).
    pub fn may_draw(&self) -> bool {
        !self.announced()
    }

    /// Unsaved drawings would be lost if the tab closed now.
    pub fn away(&self) -> bool {
        self.server != Server::Ready
    }

    pub fn banner(&self) -> Option<Banner> {
        match self.server {
            Server::ShuttingDown => Some(Banner::ShuttingDown),
            Server::Starting => Some(Banner::Starting),
            Server::Unreachable => Some(Banner::Unreachable),
            Server::Ready if self.update.is_some() => Some(Banner::Updated),
            Server::Ready => None,
        }
    }
}

fn phase(p: ServerPhase) -> Server {
    match p {
        ServerPhase::Starting => Server::Starting,
        ServerPhase::Ready => Server::Ready,
        ServerPhase::ShuttingDown => Server::ShuttingDown,
    }
}

/// The notice shown at the bottom of every page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Banner {
    ShuttingDown,
    Starting,
    Unreachable,
    /// The server runs another version: reload at a safe moment.
    Updated,
}

/// What the page is doing at the moment of a possible reload.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Moment {
    /// A part is open in the reader (reading or drawing).
    pub in_reader: bool,
    /// Annotation pages not saved yet.
    pub pending_saves: bool,
    /// Requests still running (saves, uploads).
    pub requests_running: bool,
    /// Something was typed or chosen in a form and not saved since.
    pub unsaved_input: bool,
}

/// R-OPS-048: reload into the new version only when the server is ready and
/// nothing is lost or interrupted: never in the reader (it reloads after the
/// user left it, or with "Reload now"), never with pending saves, running
/// requests or unsaved form input.
pub fn may_reload(conn: &Conn, m: Moment) -> bool {
    conn.update.is_some()
        && conn.server == Server::Ready
        && !m.in_reader
        && !m.pending_saves
        && !m.requests_running
        && !m.unsaved_input
}

/// The reader's routes (`/parts/:id`, `/programmes/:pid/parts/:id`), not its
/// editors (`/parts/:id/edit`, `/parts/:id/systems`).
pub fn is_reader_path(path: &str) -> bool {
    let s: Vec<&str> = path.trim_end_matches('/').split('/').skip(1).collect();
    let id = |x: &str| !x.is_empty() && x.bytes().all(|b| b.is_ascii_digit());
    match s.as_slice() {
        ["parts", p] => id(p),
        ["programmes", q, "parts", p] => id(q) && id(p),
        _ => false,
    }
}

pub use live::*;

/// The running parts (browser only; compiled on the host for the tests).
mod live {
    use super::*;
    use crate::{api, bridge};
    use dioxus::prelude::*;

    /// This tab's view of the server (memory only, R-SEC-001).
    pub static CONN: GlobalSignal<Conn> = Signal::global(Conn::default);
    /// Bumped on every [`Then::Flush`] / [`Then::Resend`], with the effect;
    /// the reader sends its pending pages when it changes.
    pub static EFFECT: GlobalSignal<(u64, Then)> = Signal::global(|| (0, Then::None));

    /// Feeds `input` into [`CONN`] and publishes the effect.
    pub fn report(input: Input) {
        let mut conn = CONN.peek().clone();
        let effect = conn.apply(input, musicus_api::VERSION);
        if conn != *CONN.peek() {
            bridge::set_unload_warning("away", conn.away());
            *CONN.write() = conn;
        }
        if effect != Then::None {
            let n = EFFECT.peek().0 + 1;
            *EFFECT.write() = (n, effect);
        }
    }

    /// How often the server is asked while it is away (R-OPS-046).
    const POLL_MS: u32 = 3000;

    /// Runs for the life of the app: reads the event stream, polls while the
    /// server is away, and reloads after an update at a safe moment.
    pub async fn run() {
        spawn(events());
        spawn(reload_when_safe());
        loop {
            gloo_timers::future::TimeoutFuture::new(POLL_MS).await;
            if CONN.peek().server != Server::Ready {
                poll().await;
            }
        }
    }

    /// `GET /api/health` (200 only when ready), then the version.
    async fn poll() {
        match api::health().await {
            Ok(None) => match api::version().await {
                Ok(version) => report(Input::State(ServerState { state: ServerPhase::Ready, version })),
                Err(_) => report(Input::Lost),
            },
            Ok(Some(phase)) => report(Input::Refused(phase)),
            Err(()) => report(Input::Lost),
        }
    }

    async fn events() {
        loop {
            let ev = bridge::next_server_event().await;
            match ev.as_str() {
                "lost" => report(Input::Lost),
                // The browser gave up (an error answer): check the session and reopen.
                "closed" => {
                    spawn(reopen());
                }
                data => {
                    if let Ok(state) = serde_json::from_str::<ServerState>(data) {
                        report(Input::State(state));
                    }
                }
            }
        }
    }

    async fn reopen() {
        loop {
            gloo_timers::future::TimeoutFuture::new(POLL_MS).await;
            if CONN.peek().announced() {
                continue;
            }
            match api::get::<musicus_api::CurrentUser>("/api/auth/me").await {
                Ok(_) => {
                    bridge::open_events(musicus_api::EVENTS_PATH);
                    return;
                }
                // Logged out: `api` went to the login page.
                Err(e) if e.status == 401 => return,
                Err(_) => {}
            }
        }
    }

    async fn reload_when_safe() {
        let mut last_path = String::new();
        loop {
            gloo_timers::future::TimeoutFuture::new(1000).await;
            let Some(window) = web_sys::window() else { return };
            let path = window.location().pathname().unwrap_or_default();
            if path != last_path {
                // Leaving a page drops what was typed there.
                bridge::clear_unsaved_input();
                last_path = path.clone();
            }
            let moment = Moment {
                in_reader: is_reader_path(&path),
                pending_saves: bridge::has_pending(),
                requests_running: api::running() > 0,
                unsaved_input: bridge::unsaved_input(),
            };
            if may_reload(&CONN.peek(), moment) {
                reload();
                return;
            }
        }
    }

    /// Reloads the page (the new client); pending pages go out with the
    /// `pagehide` flush of the bridge.
    pub fn reload() {
        if let Some(w) = web_sys::window() {
            let _ = w.location().reload();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OWN: &str = "1.0.0";

    fn state(state: ServerPhase, version: &str) -> Input {
        Input::State(ServerState { state, version: version.into() })
    }

    #[test]
    fn an_announced_stop_blocks_first_then_flushes() {
        let mut c = Conn::default();
        assert!(c.may_draw() && !c.announced() && c.banner().is_none());
        let effect = c.apply(state(ServerPhase::ShuttingDown, OWN), OWN);
        // The state is already blocking when the caller gets the effect.
        assert!(!c.may_draw() && c.announced());
        assert_eq!(effect, Then::Flush);
        assert_eq!(c.banner(), Some(Banner::ShuttingDown));
        // The same announcement again, an answer (the flush) or a lost
        // connection (the server is going) changes nothing and flushes not again.
        for input in [state(ServerPhase::ShuttingDown, OWN), Input::Answered, Input::Lost] {
            assert_eq!(c.apply(input, OWN), Then::None);
            assert_eq!(c.server, Server::ShuttingDown);
        }
        // A 503 `server_shutting_down` (stream missed) announces it as well.
        let mut d = Conn::default();
        assert_eq!(d.apply(Input::Refused(ServerPhase::ShuttingDown), OWN), Then::Flush);
        assert!(!d.may_draw());
    }

    #[test]
    fn starting_blocks_until_ready_then_resends() {
        let mut c = Conn { server: Server::ShuttingDown, update: None };
        assert_eq!(c.apply(Input::Refused(ServerPhase::Starting), OWN), Then::None);
        assert_eq!((c.server, c.may_draw(), c.banner()), (Server::Starting, false, Some(Banner::Starting)));
        assert_eq!(c.apply(Input::Answered, OWN), Then::None, "an answer while starting is no ready");
        assert_eq!(c.apply(state(ServerPhase::Ready, OWN), OWN), Then::Resend);
        assert!(c.may_draw() && c.banner().is_none() && c.update.is_none());
    }

    #[test]
    fn network_loss_keeps_drawing_and_recovers_on_any_answer() {
        let mut c = Conn::default();
        assert_eq!(c.apply(Input::Lost, OWN), Then::None);
        assert_eq!(c.server, Server::Unreachable);
        assert!(c.may_draw() && !c.announced() && c.away());
        assert_eq!(c.banner(), Some(Banner::Unreachable));
        assert_eq!(c.apply(Input::Answered, OWN), Then::Resend);
        assert_eq!(c.server, Server::Ready);
        assert!(!c.away());
    }

    #[test]
    fn a_start_that_fails_is_not_reachable() {
        let mut c = Conn { server: Server::Starting, update: None };
        c.apply(Input::Lost, OWN);
        assert_eq!(c.server, Server::Unreachable);
        assert!(c.may_draw());
    }

    #[test]
    fn another_version_waits_for_a_safe_moment() {
        let mut c = Conn::default();
        c.apply(state(ServerPhase::ShuttingDown, OWN), OWN);
        c.apply(state(ServerPhase::Ready, "1.1.0"), OWN);
        assert_eq!(c.update.as_deref(), Some("1.1.0"));
        assert_eq!(c.banner(), Some(Banner::Updated));
        let idle = Moment::default();
        assert!(may_reload(&c, idle));
        for busy in [
            Moment { in_reader: true, ..idle },
            Moment { pending_saves: true, ..idle },
            Moment { requests_running: true, ..idle },
            Moment { unsaved_input: true, ..idle },
        ] {
            assert!(!may_reload(&c, busy), "{busy:?}");
        }
        // Not while the server is away again.
        c.apply(Input::Lost, OWN);
        assert!(!may_reload(&c, idle));
        // The same version again: nothing to reload.
        c.apply(state(ServerPhase::Ready, OWN), OWN);
        assert!(c.update.is_none() && !may_reload(&c, idle));
    }

    #[test]
    fn reader_paths() {
        assert!(is_reader_path("/parts/12"));
        assert!(is_reader_path("/programmes/3/parts/12"));
        assert!(is_reader_path("/parts/12/"));
        for p in ["/", "/parts/12/edit", "/parts/12/systems", "/programmes/3", "/parts/x", "/parts/"] {
            assert!(!is_reader_path(p), "{p}");
        }
    }
}
