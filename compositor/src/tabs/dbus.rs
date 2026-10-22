//! The bar's tab order on the session bus (ADR-0080):
//! `com.sectorflabs.Patina.Tabs` at `/com/sectorflabs/Patina/Tabs`.
//! `Move(from, to)` moves a window in the list of open windows, as
//! wlr-foreign-toplevel announced them, from index `from` to `to` (as
//! `Vec::remove` then `insert` would), and `Moved(from, to)` says it did,
//! so the bar moves its tab to match. The pinned apps' places come from the
//! bar's config instead, which both read. Served nested too.

use std::sync::mpsc;
use std::time::Duration;

use smithay::reexports::calloop::{LoopHandle, channel};

use crate::state::State;

pub const NAME: &str = "com.sectorflabs.Patina.Tabs";
pub const PATH: &str = "/com/sectorflabs/Patina/Tabs";

/// The bus asking to move window `from` to `to`; the answer is whether
/// both are windows.
#[derive(Debug)]
pub struct Request {
    pub from: usize,
    pub to: usize,
    pub reply: mpsc::Sender<bool>,
}

/// Claims the name and serves the interface while the connection lives;
/// requests go to the loop at `handle`.
pub fn serve(handle: &LoopHandle<'static, State>) -> Result<zbus::blocking::Connection, String> {
    let (requests, incoming) = channel::channel::<Request>();
    handle
        .insert_source(incoming, |event, _, state| {
            if let channel::Event::Msg(request) = event {
                let moved = state.toplevels.move_window(request.from, request.to);
                let _ = request.reply.send(moved);
            }
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

fn failed(e: impl ToString) -> zbus::fdo::Error {
    zbus::fdo::Error::Failed(e.to_string())
}

#[zbus::interface(name = "com.sectorflabs.Patina.Tabs")]
impl Interface {
    /// Moves the window at `from` in the list of open windows to `to`,
    /// then signals `Moved`.
    #[zbus(name = "Move")]
    async fn move_window(
        &self,
        #[zbus(signal_emitter)] emitter: zbus::object_server::SignalEmitter<'_>,
        from: u32,
        to: u32,
    ) -> zbus::fdo::Result<()> {
        let (reply, answer) = mpsc::channel();
        let request = Request {
            from: from as usize,
            to: to as usize,
            reply,
        };
        self.requests.send(request).map_err(failed)?;
        match answer.recv_timeout(Duration::from_secs(1)) {
            Ok(true) => Self::moved(&emitter, from, to).await.map_err(failed),
            Ok(false) => Err(zbus::fdo::Error::InvalidArgs(format!(
                "no window at {from} or {to}"
            ))),
            Err(_) => Err(failed("the compositor didn't answer")),
        }
    }

    #[zbus(signal)]
    async fn moved(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        from: u32,
        to: u32,
    ) -> zbus::Result<()>;
}

/// On a bus of its own: `dbus-run-session -- cargo test -p
/// patina-compositor tabs -- --ignored`.
#[cfg(test)]
mod tests {
    use std::time::Instant;

    use smithay::reexports::calloop::EventLoop;

    use super::*;

    #[test]
    #[ignore = "needs a session bus of its own: run under dbus-run-session"]
    fn the_bar_moves_a_window_and_hears_it_moved() {
        let mut event_loop: EventLoop<'static, Vec<String>> = EventLoop::try_new().unwrap();
        let (send, incoming) = channel::channel::<Request>();
        event_loop
            .handle()
            .insert_source(incoming, |event, _, windows| {
                if let channel::Event::Msg(r) = event {
                    let ok = r.from < windows.len() && r.to < windows.len();
                    if ok {
                        let w = windows.remove(r.from);
                        windows.insert(r.to, w);
                    }
                    let _ = r.reply.send(ok);
                }
            })
            .unwrap();
        let _served = serve_with(send).expect("the name is free on a bus of our own");
        let (heard, moves) = mpsc::channel();
        std::thread::spawn(move || patina_system::pins::watch_moves(|m| heard.send(m).is_ok()));
        // Until the watcher is listening.
        std::thread::sleep(Duration::from_millis(200));
        let mut windows: Vec<String> = ["a", "b", "c"].map(String::from).to_vec();
        let mover = std::thread::spawn(|| {
            (
                patina_system::pins::move_window(0, 2),
                patina_system::pins::move_window(5, 0),
            )
        });
        let deadline = Instant::now() + Duration::from_millis(500);
        while Instant::now() < deadline {
            event_loop
                .dispatch(Some(Duration::from_millis(20)), &mut windows)
                .unwrap();
        }
        let (moved, past_the_end) = mover.join().unwrap();
        assert!(moved.is_ok(), "{moved:?}");
        assert!(past_the_end.is_err());
        assert_eq!(windows, ["b", "c", "a"]);
        assert_eq!(moves.try_iter().collect::<Vec<_>>(), [(0, 2)]);
    }
}
