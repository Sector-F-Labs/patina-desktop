//! The clipboard history on the session bus (ADR-0114), for the tray applet:
//! `com.sectorflabs.Patina.Clipboard` at `/com/sectorflabs/Patina/Clipboard`. Served nested
//! too, on the VM's or `make run`'s own bus.
//!
//! Built as the keyboard's is (`keyboard/dbus.rs`): zbus runs on threads of its own, a
//! request reaches the event loop over a calloop channel, and what the history holds comes
//! back as a `Status` that `List` and the properties read and `Changed` announces.
//!
//! `List` gives each entry's preview, cut short, and never its content. The content is
//! `Content`'s, which drags need (ADR-0140): an entry's bytes in one of its types, to a caller
//! that asks for an id it knows from `List`. The bus is open to every program the user runs,
//! so `List` keeps what it hands out small, and `Content` is what the history's privacy
//! rests on (ADR-0140).

use std::sync::{Arc, Mutex, mpsc};

use smithay::reexports::calloop::{LoopHandle, channel};
use zbus::object_server::SignalEmitter;

use super::{Contents, Row};
use crate::state::State;

pub const NAME: &str = "com.sectorflabs.Patina.Clipboard";
pub const PATH: &str = "/com/sectorflabs/Patina/Clipboard";

/// What the interface says.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Status {
    pub rows: Vec<Row>,
    /// What each row holds, for `Types` and `Content`.
    pub contents: Contents,
    pub enabled: bool,
    pub paused: bool,
}

/// What a program on the bus asks for, done in the event loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    Pick(u64),
    Delete(u64),
    Clear,
    Enabled(bool),
    Paused(bool),
}

/// The compositor's end: where to send what it has.
pub struct Service {
    status: Arc<Mutex<Status>>,
    changed: mpsc::Sender<()>,
}

impl Service {
    /// Claims the name and serves the interface, on threads of its own. What's asked for on
    /// the bus is done in the loop at `handle`.
    pub fn start(handle: &LoopHandle<'static, State>) -> Result<Self, String> {
        let (requests, incoming) = channel::channel::<Request>();
        handle
            .insert_source(incoming, |event, _, state| {
                if let channel::Event::Msg(request) = event {
                    match request {
                        Request::Pick(id) => state.clipboard_pick(id),
                        Request::Delete(id) => state.clipboard_delete(id),
                        Request::Clear => state.clipboard_clear(),
                        Request::Enabled(on) => state.clipboard_enable(on),
                        Request::Paused(on) => state.clipboard_pause(on),
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Self::serve(requests)
    }

    fn serve(requests: channel::Sender<Request>) -> Result<Self, String> {
        let status = Arc::new(Mutex::new(Status {
            enabled: true,
            ..Status::default()
        }));
        let (changed, announce) = mpsc::channel();
        let interface = Interface {
            requests,
            status: status.clone(),
        };
        let connection = zbus::blocking::connection::Builder::session()
            .and_then(|b| b.name(NAME))
            .and_then(|b| b.serve_at(PATH, interface))
            .and_then(|b| b.build())
            .map_err(|e| e.to_string())?;
        std::thread::Builder::new()
            .name("clipboard".into())
            .spawn(move || announce_changes(&connection, &announce))
            .map_err(|e| e.to_string())?;
        Ok(Self { status, changed })
    }

    /// The compositor's latest, announced if it changed.
    pub fn publish(&self, new: Status) {
        let Ok(mut status) = self.status.lock() else {
            return;
        };
        if *status == new {
            return;
        }
        *status = new;
        drop(status);
        let _ = self.changed.send(());
    }
}

/// Emits `Changed`, and PropertiesChanged for the two properties, when the compositor says
/// something changed.
fn announce_changes(connection: &zbus::blocking::Connection, changed: &mpsc::Receiver<()>) {
    let Ok(interface) = connection.object_server().interface::<_, Interface>(PATH) else {
        return;
    };
    while changed.recv().is_ok() {
        let emitter = interface.signal_emitter();
        let i = interface.get();
        let result = zbus::block_on(async {
            i.enabled_changed(emitter).await?;
            i.paused_changed(emitter).await?;
            Interface::changed(emitter).await
        });
        if let Err(e) = result {
            eprintln!("patina-compositor: clipboard: {e}");
        }
    }
}

fn no_entry(id: u64) -> zbus::fdo::Error {
    zbus::fdo::Error::InvalidArgs(format!("no entry {id} in the history"))
}

struct Interface {
    requests: channel::Sender<Request>,
    status: Arc<Mutex<Status>>,
}

impl Interface {
    fn status(&self) -> Status {
        self.status.lock().map(|s| s.clone()).unwrap_or_default()
    }

    fn ask(&self, request: Request) -> zbus::fdo::Result<()> {
        self.requests
            .send(request)
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }

    fn known(&self, id: u64) -> zbus::fdo::Result<()> {
        if self.status().rows.iter().any(|r| r.0 == id) {
            Ok(())
        } else {
            Err(no_entry(id))
        }
    }
}

#[zbus::interface(name = "com.sectorflabs.Patina.Clipboard")]
impl Interface {
    /// The history, newest first: each entry's id, its kind (`text`, `image` or `other`), a
    /// one-line preview, the path of its thumbnail (empty if it has none) and when it was
    /// copied, in seconds since 1970.
    fn list(&self) -> Vec<Row> {
        self.status().rows
    }

    /// The types an entry holds, as MIME types (`text/plain;charset=utf-8`, `image/png`), to
    /// say what a drag of it offers.
    fn types(&self, id: u64) -> zbus::fdo::Result<Vec<String>> {
        let status = self.status();
        let types = status.contents.get(id).ok_or_else(|| no_entry(id))?;
        Ok(types.iter().map(|(mime, _)| mime.clone()).collect())
    }

    /// An entry's bytes in one of its types, for a drag to hand to the app it's dropped on.
    fn content(&self, id: u64, mime: &str) -> zbus::fdo::Result<Vec<u8>> {
        let status = self.status();
        let types = status.contents.get(id).ok_or_else(|| no_entry(id))?;
        types
            .iter()
            .find(|(m, _)| m == mime)
            .map(|(_, bytes)| bytes.clone())
            .ok_or_else(|| {
                zbus::fdo::Error::InvalidArgs(format!("entry {id} has nothing as {mime}"))
            })
    }

    /// Puts an entry on the clipboard, as the newest.
    fn pick(&self, id: u64) -> zbus::fdo::Result<()> {
        self.known(id)?;
        self.ask(Request::Pick(id))
    }

    /// Forgets an entry.
    fn delete(&self, id: u64) -> zbus::fdo::Result<()> {
        self.known(id)?;
        self.ask(Request::Delete(id))
    }

    /// Forgets every entry, and empties the clipboard if it holds one of them.
    fn clear(&self) -> zbus::fdo::Result<()> {
        self.ask(Request::Clear)
    }

    /// The history changed: listeners call `List` again.
    #[zbus(signal)]
    async fn changed(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;

    /// Whether copies are being kept at all.
    #[zbus(property)]
    fn enabled(&self) -> bool {
        self.status().enabled
    }

    #[zbus(property)]
    fn set_enabled(&self, on: bool) -> zbus::fdo::Result<()> {
        self.ask(Request::Enabled(on))
    }

    /// Whether new copies are being left out, with what's there kept.
    #[zbus(property)]
    fn paused(&self) -> bool {
        self.status().paused
    }

    #[zbus(property)]
    fn set_paused(&self, on: bool) -> zbus::fdo::Result<()> {
        self.ask(Request::Paused(on))
    }
}

/// On a bus of their own: `dbus-run-session -- cargo test -p patina-compositor clipboard --
/// --ignored`.
#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use smithay::reexports::calloop::EventLoop;

    use super::*;

    fn call(
        method: &str,
        body: &(impl serde::Serialize + zbus::zvariant::DynamicType),
    ) -> zbus::Result<zbus::Message> {
        let c = zbus::blocking::Connection::session()?;
        c.call_method(Some(NAME), PATH, Some(NAME), method, body)
    }

    #[test]
    #[ignore = "needs a session bus of its own: run under dbus-run-session"]
    fn clients_read_the_history_and_ask_for_changes() {
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
        let service = Service::serve(send).expect("the name is free on a bus of our own");
        let held: Contents = Contents::from_entries([(
            2,
            Arc::new(vec![(
                "text/plain;charset=utf-8".to_string(),
                b"second".to_vec(),
            )]),
        )]);
        service.publish(Status {
            contents: held,
            rows: vec![
                (2, "text".into(), "second".into(), String::new(), 20),
                (
                    1,
                    "image".into(),
                    "Image (png, 2 KB)".into(),
                    String::new(),
                    10,
                ),
            ],
            enabled: true,
            paused: false,
        });

        let rows: Vec<Row> = call("List", &()).unwrap().body().deserialize().unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].2, "second");
        assert_eq!(rows[1].1, "image");

        let types: Vec<String> = call("Types", &(2u64,))
            .unwrap()
            .body()
            .deserialize()
            .unwrap();
        assert_eq!(types, ["text/plain;charset=utf-8"]);
        let bytes: Vec<u8> = call("Content", &(2u64, "text/plain;charset=utf-8"))
            .unwrap()
            .body()
            .deserialize()
            .unwrap();
        assert_eq!(bytes, b"second");
        assert!(
            call("Content", &(2u64, "image/png")).is_err(),
            "not in that type"
        );
        assert!(
            call("Content", &(99u64, "text/plain")).is_err(),
            "there's no entry 99"
        );

        call("Pick", &(2u64,)).unwrap();
        call("Delete", &(1u64,)).unwrap();
        call("Clear", &()).unwrap();
        assert!(call("Pick", &(99u64,)).is_err(), "there's no entry 99");

        let c = zbus::blocking::Connection::session().unwrap();
        c.call_method(
            Some(NAME),
            PATH,
            Some("org.freedesktop.DBus.Properties"),
            "Set",
            &(NAME, "Paused", zbus::zvariant::Value::from(true)),
        )
        .unwrap();

        let mut got = Vec::new();
        let deadline = Instant::now() + Duration::from_millis(300);
        while Instant::now() < deadline {
            event_loop
                .dispatch(Some(Duration::from_millis(20)), &mut got)
                .unwrap();
        }
        assert_eq!(
            got,
            [
                Request::Pick(2),
                Request::Delete(1),
                Request::Clear,
                Request::Paused(true)
            ]
        );
    }
}
