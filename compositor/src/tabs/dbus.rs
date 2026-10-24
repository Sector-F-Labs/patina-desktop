//! The bar's tab order on the session bus (ADR-0080):
//! `com.sectorflabs.Patina.Tabs` at `/com/sectorflabs/Patina/Tabs`.
//! `Move(from, to)` moves a window in the list of open windows, as
//! wlr-foreign-toplevel announced them, from index `from` to `to` (as
//! `Vec::remove` then `insert` would), and `Moved(from, to)` says it did,
//! so the bar moves its tab to match. The pinned apps' places come from the
//! bar's config instead, which both read. Served nested too.
//!
//! The process manager reads the same windows here (ADR-0088): `Windows()`
//! gives each one's app id, title and process, which wlr-foreign-toplevel
//! doesn't, and `Activate(pid)` brings one of a process's forward.

use std::sync::mpsc;
use std::time::Duration;

use smithay::reexports::calloop::{LoopHandle, channel};

use crate::state::State;

pub const NAME: &str = "com.sectorflabs.Patina.Tabs";
pub const PATH: &str = "/com/sectorflabs/Patina/Tabs";

/// What the bus asks of the loop.
#[derive(Debug)]
pub enum Request {
    /// Move window `from` to `to`; the answer is whether both are windows.
    Move {
        from: usize,
        to: usize,
        reply: mpsc::Sender<bool>,
    },
    /// The windows, as app id, title and process.
    Windows(mpsc::Sender<Vec<(String, String, u32)>>),
    /// Bring forward a window of this process; whether it has one.
    Activate(u32, mpsc::Sender<bool>),
}

/// Claims the name and serves the interface while the connection lives;
/// requests go to the loop at `handle`.
pub fn serve(handle: &LoopHandle<'static, State>) -> Result<zbus::blocking::Connection, String> {
    let (requests, incoming) = channel::channel::<Request>();
    handle
        .insert_source(incoming, |event, _, state| {
            let channel::Event::Msg(request) = event else {
                return;
            };
            match request {
                Request::Move { from, to, reply } => {
                    let _ = reply.send(state.toplevels.move_window(from, to));
                }
                Request::Windows(reply) => {
                    let windows = state.windows_with_pids();
                    let _ = reply.send(windows.into_iter().map(|(_, a, t, p)| (a, t, p)).collect());
                }
                Request::Activate(pid, reply) => {
                    let _ = reply.send(state.activate_pid(pid));
                }
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
        let request = Request::Move {
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

    /// The open windows in the bar's order: app id, title and process ID.
    fn windows(&self) -> zbus::fdo::Result<Vec<(String, String, u32)>> {
        let (reply, answer) = mpsc::channel();
        self.requests
            .send(Request::Windows(reply))
            .map_err(failed)?;
        answer
            .recv_timeout(Duration::from_secs(1))
            .map_err(|_| failed("the compositor didn't answer"))
    }

    /// Brings forward a window of process `pid`, as its tab would.
    fn activate(&self, pid: u32) -> zbus::fdo::Result<()> {
        let (reply, answer) = mpsc::channel();
        self.requests
            .send(Request::Activate(pid, reply))
            .map_err(failed)?;
        match answer.recv_timeout(Duration::from_secs(1)) {
            Ok(true) => Ok(()),
            Ok(false) => Err(zbus::fdo::Error::InvalidArgs(format!(
                "process {pid} has no window"
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

    /// One test at a time holds the name.
    static BUS: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    #[ignore = "needs a session bus of its own: run under dbus-run-session"]
    fn the_bar_moves_a_window_and_hears_it_moved() {
        let _bus = BUS.lock().unwrap_or_else(|e| e.into_inner());
        let mut event_loop: EventLoop<'static, Vec<String>> = EventLoop::try_new().unwrap();
        let (send, incoming) = channel::channel::<Request>();
        event_loop
            .handle()
            .insert_source(incoming, |event, _, windows| {
                if let channel::Event::Msg(Request::Move { from, to, reply }) = event {
                    let ok = from < windows.len() && to < windows.len();
                    if ok {
                        let w = windows.remove(from);
                        windows.insert(to, w);
                    }
                    let _ = reply.send(ok);
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

    #[test]
    #[ignore = "needs a session bus of its own: run under dbus-run-session"]
    fn the_process_manager_reads_the_windows_and_activates_one() {
        let _bus = BUS.lock().unwrap_or_else(|e| e.into_inner());
        let mut event_loop: EventLoop<'static, Vec<u32>> = EventLoop::try_new().unwrap();
        let (send, incoming) = channel::channel::<Request>();
        event_loop
            .handle()
            .insert_source(incoming, |event, _, activated| match event {
                channel::Event::Msg(Request::Windows(reply)) => {
                    let _ = reply.send(vec![("firefox".into(), "Mozilla Firefox".into(), 30)]);
                }
                channel::Event::Msg(Request::Activate(pid, reply)) => {
                    activated.push(pid);
                    let _ = reply.send(pid == 30);
                }
                _ => {}
            })
            .unwrap();
        let _served = serve_with(send).expect("the name is free on a bus of our own");
        let asker = std::thread::spawn(|| {
            let windows = patina_system::windows::Windows::connect().unwrap();
            (windows.list(), windows.activate(30), windows.activate(31))
        });
        let mut activated = Vec::new();
        let deadline = Instant::now() + Duration::from_millis(500);
        while Instant::now() < deadline {
            event_loop
                .dispatch(Some(Duration::from_millis(20)), &mut activated)
                .unwrap();
        }
        let (list, thirty, other) = asker.join().unwrap();
        let list = list.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!((list[0].app_id.as_str(), list[0].pid), ("firefox", 30));
        assert!(thirty.is_ok(), "{thirty:?}");
        assert!(other.is_err(), "31 has no window");
        assert_eq!(activated, [30, 31]);
    }
}
