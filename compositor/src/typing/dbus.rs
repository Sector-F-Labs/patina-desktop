//! Typing on the session bus (ADR-0071), for dictation:
//! `com.sectorflabs.Patina.Input` at `/com/sectorflabs/Patina/Input`, with
//! `Commit(text)`, which types `text` into the focused window. Served nested
//! too.

use smithay::reexports::calloop::{LoopHandle, channel};

use crate::state::State;

pub const NAME: &str = "com.sectorflabs.Patina.Input";
pub const PATH: &str = "/com/sectorflabs/Patina/Input";

/// Claims the name and serves the interface while the connection lives;
/// texts go to the loop at `handle`.
pub fn serve(handle: &LoopHandle<'static, State>) -> Result<zbus::blocking::Connection, String> {
    let (texts, incoming) = channel::channel::<String>();
    handle
        .insert_source(incoming, |event, _, state| {
            if let channel::Event::Msg(text) = event {
                state.commit_text(text);
            }
        })
        .map_err(|e| e.to_string())?;
    serve_with(texts)
}

fn serve_with(texts: channel::Sender<String>) -> Result<zbus::blocking::Connection, String> {
    zbus::blocking::connection::Builder::session()
        .and_then(|b| b.name(NAME))
        .and_then(|b| b.serve_at(PATH, Interface { texts }))
        .and_then(|b| b.build())
        .map_err(|e| e.to_string())
}

struct Interface {
    texts: channel::Sender<String>,
}

#[zbus::interface(name = "com.sectorflabs.Patina.Input")]
impl Interface {
    /// Types `text` into the focused window, through the clipboard, which
    /// is given back as it was after.
    fn commit(&self, text: String) -> zbus::fdo::Result<()> {
        self.texts
            .send(text)
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }
}

/// On a bus of its own: `dbus-run-session -- cargo test -p
/// patina-compositor typing -- --ignored`.
#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use smithay::reexports::calloop::EventLoop;

    use super::*;

    #[test]
    #[ignore = "needs a session bus of its own: run under dbus-run-session"]
    fn texts_reach_the_loop() {
        let mut event_loop: EventLoop<'static, Vec<String>> = EventLoop::try_new().unwrap();
        let (send, incoming) = channel::channel::<String>();
        event_loop
            .handle()
            .insert_source(incoming, |event, _, got| {
                if let channel::Event::Msg(text) = event {
                    got.push(text);
                }
            })
            .unwrap();
        let _served = serve_with(send).expect("the name is free on a bus of our own");
        let bus = zbus::blocking::Connection::session().unwrap();
        bus.call_method(Some(NAME), PATH, Some(NAME), "Commit", &("hej då",))
            .unwrap();
        let mut got = Vec::new();
        let deadline = Instant::now() + Duration::from_millis(300);
        while Instant::now() < deadline {
            event_loop
                .dispatch(Some(Duration::from_millis(20)), &mut got)
                .unwrap();
        }
        assert_eq!(got, ["hej då"]);
    }
}
