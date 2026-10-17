//! The session on the session bus (ADR-0048), for the bar's power menu:
//! `com.sectorflabs.Patina.Session` at `/com/sectorflabs/Patina/Session`,
//! with `Lock()`, which runs the `[lock]` command, and `LogOut()`, which
//! quits the compositor cleanly, so patina-session ends and the login screen
//! comes back. Served nested too, where logging out closes the window.

use smithay::reexports::calloop::{LoopHandle, channel};

use crate::state::State;

pub const NAME: &str = "com.sectorflabs.Patina.Session";
pub const PATH: &str = "/com/sectorflabs/Patina/Session";

/// What the bus asks of the compositor.
#[derive(Debug, PartialEq)]
pub enum Request {
    Lock,
    LogOut,
}

/// Claims the name and serves the interface while the connection lives;
/// requests go to the loop at `handle`.
pub fn serve(handle: &LoopHandle<'static, State>) -> Result<zbus::blocking::Connection, String> {
    let (requests, incoming) = channel::channel::<Request>();
    handle
        .insert_source(incoming, |event, _, state| match event {
            channel::Event::Msg(Request::Lock) => state.lock_screen(),
            channel::Event::Msg(Request::LogOut) => state.log_out(),
            channel::Event::Closed => {}
        })
        .map_err(|e| e.to_string())?;
    serve_with(requests)
}

fn serve_with(requests: channel::Sender<Request>) -> Result<zbus::blocking::Connection, String> {
    zbus::blocking::connection::Builder::session()
        .and_then(|b| b.name(NAME))
        .and_then(|b| b.serve_at(PATH, Interface { requests }))
        .and_then(|b| b.build())
        .map_err(|e| e.to_string())
}

struct Interface {
    requests: channel::Sender<Request>,
}

#[zbus::interface(name = "com.sectorflabs.Patina.Session")]
impl Interface {
    /// Locks the screen with the `[lock]` command, unless it's locked.
    fn lock(&self) -> zbus::fdo::Result<()> {
        self.requests
            .send(Request::Lock)
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }

    /// Ends the session: the compositor quits, and every app with it.
    fn log_out(&self) -> zbus::fdo::Result<()> {
        self.requests
            .send(Request::LogOut)
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }
}

/// On a bus of its own: `dbus-run-session -- cargo test -p
/// patina-compositor session -- --ignored`.
#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use smithay::reexports::calloop::EventLoop;

    use super::*;

    #[test]
    #[ignore = "needs a session bus of its own: run under dbus-run-session"]
    fn the_power_menu_locks_and_logs_out() {
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
        let _served = serve_with(send).expect("the name is free on a bus of our own");
        assert!(patina_system::session::abilities().session);
        patina_system::session::lock().unwrap();
        patina_system::session::log_out().unwrap();
        let mut got = Vec::new();
        let deadline = Instant::now() + Duration::from_millis(300);
        while Instant::now() < deadline {
            event_loop
                .dispatch(Some(Duration::from_millis(20)), &mut got)
                .unwrap();
        }
        assert_eq!(got, [Request::Lock, Request::LogOut]);
    }
}
