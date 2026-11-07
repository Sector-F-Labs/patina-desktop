//! The monitors on the session bus (ADR-0121):
//! `com.sectorflabs.Patina.Displays` at `/com/sectorflabs/Patina/Displays`.
//!
//! Methods: `List()` the monitors, each a dictionary (see [`Monitor`]);
//! `Saved()` the displays remembered, each a dictionary, and `Forget(id)`;
//! `Mode()` and `SetMode(mode)`: `extend` or `mirror` drive every connected
//! monitor that way, `off` drives the primary alone; `SetPlace(id, to,
//! side, offset)` puts monitor `id` on `side` (`above`, `below`, `left` or
//! `right`) of monitor `to`, `offset` points along the edge, and an empty
//! side puts it after the one before. Signals: `Changed()` when the monitors
//! or their places change, and `MonitorAdded(id, name, known)` when one is
//! plugged in, `known` if it has been placed before; it stays off until
//! `SetMode`.
//!
//! zbus runs on threads of its own; requests reach the event loop over a
//! calloop channel and are answered from it.

use std::collections::HashMap;
use std::sync::mpsc;
use std::time::Duration;

use smithay::reexports::calloop::{LoopHandle, channel};
use zbus::zvariant::{OwnedValue, Value};

use crate::state::State;

pub const NAME: &str = "com.sectorflabs.Patina.Displays";
pub const PATH: &str = "/com/sectorflabs/Patina/Displays";

/// A monitor as listed.
#[derive(Debug, Clone, PartialEq)]
pub struct Monitor {
    /// The connector's name: eDP-1, DP-3.
    pub name: String,
    /// Stays when it's plugged in elsewhere (`edid::Edid::id`).
    pub id: String,
    pub make: String,
    pub model: String,
    /// The mode, in pixels.
    pub width: i32,
    pub height: i32,
    /// Where it is, and how large, in the desktop's points.
    pub x: i32,
    pub y: i32,
    pub logical_width: i32,
    pub logical_height: i32,
    pub scale: f64,
    pub primary: bool,
    /// It has been placed before.
    pub known: bool,
    /// It's driven; otherwise it's connected and waiting to be told to
    /// extend or mirror, or switched off.
    pub active: bool,
    /// Where it was put, if it was: next to monitor `to`, on `side`.
    pub to: String,
    pub side: String,
    pub offset: i32,
}

impl Monitor {
    fn dictionary(&self) -> HashMap<String, OwnedValue> {
        fn v<'a>(v: impl Into<Value<'a>>) -> OwnedValue {
            OwnedValue::try_from(v.into()).expect("plain values have no file descriptors")
        }
        HashMap::from([
            ("name".into(), v(self.name.as_str())),
            ("id".into(), v(self.id.as_str())),
            ("make".into(), v(self.make.as_str())),
            ("model".into(), v(self.model.as_str())),
            ("width".into(), v(self.width)),
            ("height".into(), v(self.height)),
            ("x".into(), v(self.x)),
            ("y".into(), v(self.y)),
            ("logical-width".into(), v(self.logical_width)),
            ("logical-height".into(), v(self.logical_height)),
            ("scale".into(), v(self.scale)),
            ("primary".into(), v(self.primary)),
            ("known".into(), v(self.known)),
            ("active".into(), v(self.active)),
            ("to".into(), v(self.to.as_str())),
            ("side".into(), v(self.side.as_str())),
            ("offset".into(), v(self.offset)),
        ])
    }
}

/// A saved display, for the Displays page (ADR-0124).
#[derive(Debug, Clone, PartialEq)]
pub struct SavedDisplay {
    pub id: String,
    /// What it was called when it was saved.
    pub name: String,
    /// A connected monitor goes by it.
    pub in_use: bool,
    /// The monitor it's next to, by id and by name; empty for none.
    pub to: String,
    pub to_name: String,
    pub side: String,
    pub offset: i32,
}

impl SavedDisplay {
    fn dictionary(&self) -> HashMap<String, OwnedValue> {
        fn v<'a>(v: impl Into<Value<'a>>) -> OwnedValue {
            OwnedValue::try_from(v.into()).expect("plain values have no file descriptors")
        }
        HashMap::from([
            ("id".into(), v(self.id.as_str())),
            ("name".into(), v(self.name.as_str())),
            ("in-use".into(), v(self.in_use)),
            ("to".into(), v(self.to.as_str())),
            ("to-name".into(), v(self.to_name.as_str())),
            ("side".into(), v(self.side.as_str())),
            ("offset".into(), v(self.offset)),
        ])
    }
}

type Reply<T> = mpsc::Sender<T>;

/// What the bus asks of the compositor.
#[derive(Debug)]
pub enum Request {
    List(Reply<Vec<Monitor>>),
    Saved(Reply<Vec<SavedDisplay>>),
    Forget(String, Reply<Result<(), String>>),
    Mode(Reply<String>),
    SetMode(String, Reply<Result<(), String>>),
    SetPlace {
        id: String,
        to: String,
        side: String,
        offset: i32,
        reply: Reply<Result<(), String>>,
    },
}

/// The compositor's end, for the signals.
pub struct Service {
    connection: zbus::blocking::Connection,
}

impl Service {
    /// Claims the name and serves the interface, on threads of its own.
    pub fn start(handle: &LoopHandle<'static, State>) -> Result<Self, String> {
        let (requests, incoming) = channel::channel::<Request>();
        handle
            .insert_source(incoming, |event, _, state| {
                if let channel::Event::Msg(request) = event {
                    state.displays_request(request);
                }
            })
            .map_err(|e| e.to_string())?;
        let connection = zbus::blocking::connection::Builder::session()
            .and_then(|b| b.name(NAME))
            .and_then(|b| b.serve_at(PATH, Interface { requests }))
            .and_then(|b| b.build())
            .map_err(|e| e.to_string())?;
        Ok(Self { connection })
    }

    fn emit(&self, signal: &str, body: &(impl serde::Serialize + zbus::zvariant::DynamicType)) {
        if let Err(e) = self
            .connection
            .emit_signal(None::<&str>, PATH, NAME, signal, body)
        {
            eprintln!("patina-compositor: displays: {signal}: {e}");
        }
    }

    /// The monitors or their places changed.
    pub fn changed(&self) {
        self.emit("Changed", &());
    }

    /// A monitor was plugged in.
    pub fn added(&self, id: &str, name: &str, known: bool) {
        self.emit("MonitorAdded", &(id, name, known));
    }
}

struct Interface {
    requests: channel::Sender<Request>,
}

fn failed(e: impl std::fmt::Display) -> zbus::fdo::Error {
    zbus::fdo::Error::Failed(e.to_string())
}

impl Interface {
    /// Asks the loop, and waits a moment for its answer.
    fn ask<T>(&self, request: impl FnOnce(Reply<T>) -> Request) -> zbus::fdo::Result<T> {
        let (reply, answer) = mpsc::channel();
        self.requests.send(request(reply)).map_err(failed)?;
        answer
            .recv_timeout(Duration::from_secs(1))
            .map_err(|_| failed("the compositor didn't answer"))
    }
}

#[zbus::interface(name = "com.sectorflabs.Patina.Displays")]
impl Interface {
    /// The monitors, the driven ones first, the primary first of them; each
    /// a dictionary: `name`, `id`,
    /// `make`, `model`, `width`, `height` (the mode, pixels), `x`, `y`,
    /// `logical-width`, `logical-height` (points), `scale`, `primary`,
    /// `known`, `active`, and where it was put: `to`, `side`, `offset`.
    fn list(&self) -> zbus::fdo::Result<Vec<HashMap<String, OwnedValue>>> {
        Ok(self
            .ask(Request::List)?
            .iter()
            .map(Monitor::dictionary)
            .collect())
    }

    /// The saved displays, the one saved last at the end; each a dictionary:
    /// `id`, `name`, `in-use` (a connected monitor goes by it), and where it
    /// was put: `to`, `to-name`, `side`, `offset`.
    fn saved(&self) -> zbus::fdo::Result<Vec<HashMap<String, OwnedValue>>> {
        Ok(self
            .ask(Request::Saved)?
            .iter()
            .map(SavedDisplay::dictionary)
            .collect())
    }

    /// Forgets the saved display `id`.
    fn forget(&self, id: String) -> zbus::fdo::Result<()> {
        self.ask(|reply| Request::Forget(id, reply))?
            .map_err(zbus::fdo::Error::InvalidArgs)
    }

    /// `extend`, `mirror` or `off`.
    fn mode(&self) -> zbus::fdo::Result<String> {
        self.ask(Request::Mode)
    }

    fn set_mode(&self, mode: String) -> zbus::fdo::Result<()> {
        self.ask(|reply| Request::SetMode(mode, reply))?
            .map_err(zbus::fdo::Error::InvalidArgs)
    }

    /// Puts monitor `id` on `side` of monitor `to`.
    fn set_place(&self, id: String, to: String, side: String, offset: i32) -> zbus::fdo::Result<()> {
        self.ask(|reply| Request::SetPlace {
            id,
            to,
            side,
            offset,
            reply,
        })?
        .map_err(zbus::fdo::Error::InvalidArgs)
    }

    #[zbus(signal)]
    async fn changed(emitter: &zbus::object_server::SignalEmitter<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn monitor_added(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        id: &str,
        name: &str,
        known: bool,
    ) -> zbus::Result<()>;
}
