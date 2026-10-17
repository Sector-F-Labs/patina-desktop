//! `org.freedesktop.ScreenSaver` on the session bus (ADR-0047), at
//! `/org/freedesktop/ScreenSaver` and `/ScreenSaver`, as desktops serve it:
//! apps that play (Steam while a game runs, SDL games, video players under
//! X) hold the screen on with Inhibit until UnInhibit, or until they leave
//! the bus. Nested in a desktop that already serves it, the name is taken,
//! and that desktop's applies.
//!
//! As with Night Light's, zbus runs on threads of its own and requests reach
//! the event loop over a calloop channel.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use smithay::reexports::calloop::{LoopHandle, channel};
use zbus::message::Header;

use crate::state::State;

pub const NAME: &str = "org.freedesktop.ScreenSaver";
const PATHS: [&str; 2] = ["/org/freedesktop/ScreenSaver", "/ScreenSaver"];

/// An Inhibit held by someone on the bus.
#[derive(Debug, Clone, PartialEq)]
pub struct Held {
    pub cookie: u32,
    /// Its unique bus name, to drop it if it leaves.
    pub owner: String,
    pub app: String,
    pub reason: String,
}

/// What the bus asks of the compositor.
#[derive(Debug)]
pub enum Request {
    Inhibit(Held),
    Uninhibit {
        cookie: u32,
        owner: String,
    },
    /// A name left the bus; what it held goes with it.
    Gone(String),
    /// SimulateUserActivity: as if there were input.
    Activity,
}

/// The compositor's end: whether the screen is off, for GetActive.
pub struct Service {
    off: Arc<AtomicBool>,
    _connection: zbus::blocking::Connection,
}

impl Service {
    /// Claims the name and serves the interface on both paths, on threads
    /// of its own; requests come into the loop at `handle`.
    pub fn start(handle: &LoopHandle<'static, State>) -> Result<Self, String> {
        let (requests, incoming) = channel::channel::<Request>();
        handle
            .insert_source(incoming, |event, _, state| {
                if let channel::Event::Msg(request) = event {
                    state.idle_request(request);
                }
            })
            .map_err(|e| e.to_string())?;
        Self::serve(requests)
    }

    fn serve(requests: channel::Sender<Request>) -> Result<Self, String> {
        let off = Arc::new(AtomicBool::new(false));
        let next = Arc::new(AtomicU32::new(1));
        let mut builder = zbus::blocking::connection::Builder::session()
            .and_then(|b| b.name(NAME))
            .map_err(|e| e.to_string())?;
        for path in PATHS {
            let interface = Interface {
                requests: requests.clone(),
                off: off.clone(),
                next: next.clone(),
            };
            builder = builder
                .serve_at(path, interface)
                .map_err(|e| e.to_string())?;
        }
        let connection = builder.build().map_err(|e| e.to_string())?;
        let watched = connection.clone();
        std::thread::Builder::new()
            .name("screensaver-owners".into())
            .spawn(move || watch_owners(&watched, &requests))
            .map_err(|e| e.to_string())?;
        Ok(Self {
            off,
            _connection: connection,
        })
    }

    pub fn publish(&self, off: bool) {
        self.off.store(off, Ordering::Relaxed);
    }
}

/// Drops what's held by names that leave the bus.
fn watch_owners(connection: &zbus::blocking::Connection, requests: &channel::Sender<Request>) {
    let owners = zbus::blocking::fdo::DBusProxy::new(connection)
        .and_then(|bus| bus.receive_name_owner_changed());
    let owners = match owners {
        Ok(o) => o,
        Err(e) => {
            eprintln!("patina-compositor: the screensaver can't watch the bus: {e}");
            return;
        }
    };
    for signal in owners {
        let Ok(args) = signal.args() else { continue };
        if args.new_owner().is_none()
            && args.name().starts_with(':')
            && requests
                .send(Request::Gone(args.name().to_string()))
                .is_err()
        {
            return;
        }
    }
}

struct Interface {
    requests: channel::Sender<Request>,
    off: Arc<AtomicBool>,
    next: Arc<AtomicU32>,
}

fn sender(header: &Header<'_>) -> zbus::fdo::Result<String> {
    header
        .sender()
        .map(|s| s.to_string())
        .ok_or_else(|| zbus::fdo::Error::Failed("no sender".into()))
}

fn failed(e: impl std::fmt::Display) -> zbus::fdo::Error {
    zbus::fdo::Error::Failed(e.to_string())
}

#[zbus::interface(name = "org.freedesktop.ScreenSaver")]
impl Interface {
    /// Holds the screen on until `UnInhibit` with the cookie, or until the
    /// caller leaves the bus.
    fn inhibit(
        &self,
        application_name: String,
        reason_for_inhibit: String,
        #[zbus(header)] header: Header<'_>,
    ) -> zbus::fdo::Result<u32> {
        let cookie = self.next.fetch_add(1, Ordering::Relaxed);
        self.requests
            .send(Request::Inhibit(Held {
                cookie,
                owner: sender(&header)?,
                app: application_name,
                reason: reason_for_inhibit,
            }))
            .map_err(failed)?;
        Ok(cookie)
    }

    #[zbus(name = "UnInhibit")]
    fn uninhibit(&self, cookie: u32, #[zbus(header)] header: Header<'_>) -> zbus::fdo::Result<()> {
        let owner = sender(&header)?;
        self.requests
            .send(Request::Uninhibit { cookie, owner })
            .map_err(failed)
    }

    /// As if there were input: the screen's wait starts again, and it comes
    /// back on if it was off.
    fn simulate_user_activity(&self) -> zbus::fdo::Result<()> {
        self.requests.send(Request::Activity).map_err(failed)
    }

    /// Whether the screen is off.
    fn get_active(&self) -> bool {
        self.off.load(Ordering::Relaxed)
    }
}

/// On a bus of their own: `dbus-run-session -- cargo test -p
/// patina-compositor idle -- --ignored`.
#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use smithay::reexports::calloop::EventLoop;

    use super::*;

    fn requests(event_loop: &mut EventLoop<'static, Vec<Request>>) -> Vec<Request> {
        let mut got = Vec::new();
        let deadline = Instant::now() + Duration::from_millis(300);
        while Instant::now() < deadline {
            event_loop
                .dispatch(Some(Duration::from_millis(20)), &mut got)
                .unwrap();
        }
        got
    }

    #[test]
    #[ignore = "needs a session bus of its own: run under dbus-run-session"]
    fn apps_inhibit_on_either_path_until_they_let_go_or_leave() {
        let mut event_loop: EventLoop<'static, Vec<Request>> = EventLoop::try_new().unwrap();
        let (send, incoming) = channel::channel::<Request>();
        event_loop
            .handle()
            .insert_source(incoming, |event, _, got| {
                if let channel::Event::Msg(r) = event {
                    got.push(r);
                }
            })
            .unwrap();
        let _service = Service::serve(send).expect("the name is free on a bus of our own");

        let app = zbus::blocking::Connection::session().unwrap();
        let unique = app.unique_name().unwrap().to_string();
        let proxy = |path| zbus::blocking::Proxy::new(&app, NAME, path, NAME).unwrap();
        let cookie: u32 = proxy("/ScreenSaver")
            .call("Inhibit", &("Steam", "game"))
            .unwrap();
        let other: u32 = proxy("/org/freedesktop/ScreenSaver")
            .call("Inhibit", &("mpv", "video"))
            .unwrap();
        assert_ne!(cookie, other);
        let got = requests(&mut event_loop);
        assert!(
            matches!(&got[..], [Request::Inhibit(a), Request::Inhibit(b)]
                if a.app == "Steam" && a.reason == "game" && a.owner == unique && b.cookie == other),
            "{got:?}"
        );
        proxy("/ScreenSaver")
            .call::<_, _, ()>("UnInhibit", &(cookie,))
            .unwrap();
        proxy("/ScreenSaver")
            .call::<_, _, ()>("SimulateUserActivity", &())
            .unwrap();
        let active: bool = proxy("/ScreenSaver").call("GetActive", &()).unwrap();
        assert!(!active);
        drop(app);
        let got = requests(&mut event_loop);
        assert!(
            matches!(&got[..], [Request::Uninhibit { cookie: c, .. }, Request::Activity, Request::Gone(g)]
                if *c == cookie && *g == unique),
            "{got:?}"
        );
    }
}
