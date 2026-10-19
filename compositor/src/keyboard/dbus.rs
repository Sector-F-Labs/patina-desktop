//! The keyboard's layouts on the session bus (ADR-0068), for the bar's
//! layout indicator and Settings' Keyboard page:
//! `com.sectorflabs.Patina.Keyboard` at `/com/sectorflabs/Patina/Keyboard`.
//! Served nested too, on the VM's or `make run`'s own bus.
//!
//! zbus runs on threads of its own. A choice reaches the event loop over a
//! calloop channel; the layouts the compositor has come back as a
//! `Status`, which the properties read and announce.

use std::sync::{Arc, Mutex, mpsc};

use smithay::reexports::calloop::{LoopHandle, channel};

use crate::state::State;

pub const NAME: &str = "com.sectorflabs.Patina.Keyboard";
pub const PATH: &str = "/com/sectorflabs/Patina/Keyboard";

/// One of the keymap's layouts: its xkb layout and variant names, and the
/// name xkb gives it ("English (US)").
pub type Layout = (String, String, String);

/// What the properties say.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Status {
    pub layouts: Vec<Layout>,
    /// The index of the one in use.
    pub current: u32,
}

/// The compositor's end: where to send what it has.
pub struct Service {
    status: Arc<Mutex<Status>>,
    changed: mpsc::Sender<()>,
}

impl Service {
    /// Claims the name and serves the interface, on threads of its own. A
    /// layout chosen on the bus is set in the loop at `handle`.
    pub fn start(handle: &LoopHandle<'static, State>) -> Result<Self, String> {
        let (requests, incoming) = channel::channel::<u32>();
        handle
            .insert_source(incoming, |event, _, state| {
                if let channel::Event::Msg(index) = event {
                    state.choose_layout(index);
                }
            })
            .map_err(|e| e.to_string())?;
        Self::serve(requests)
    }

    fn serve(requests: channel::Sender<u32>) -> Result<Self, String> {
        let status = Arc::new(Mutex::new(Status::default()));
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
            .name("keyboard".into())
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

/// Emits PropertiesChanged when the compositor says something changed.
fn announce_changes(connection: &zbus::blocking::Connection, changed: &mpsc::Receiver<()>) {
    let Ok(interface) = connection.object_server().interface::<_, Interface>(PATH) else {
        return;
    };
    while changed.recv().is_ok() {
        let emitter = interface.signal_emitter();
        let i = interface.get();
        let result = zbus::block_on(async {
            i.layouts_changed(emitter).await?;
            i.current_changed(emitter).await
        });
        if let Err(e) = result {
            eprintln!("patina-compositor: keyboard: {e}");
        }
    }
}

struct Interface {
    requests: channel::Sender<u32>,
    status: Arc<Mutex<Status>>,
}

impl Interface {
    fn status(&self) -> Status {
        self.status.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

#[zbus::interface(name = "com.sectorflabs.Patina.Keyboard")]
impl Interface {
    /// Switches to layout `index`, as Ctrl+Space steps to the next.
    fn set_current(&self, index: u32) -> zbus::fdo::Result<()> {
        let count = self.status().layouts.len();
        if index as usize >= count {
            return Err(zbus::fdo::Error::InvalidArgs(format!(
                "layout {index}: there are {count}"
            )));
        }
        self.requests
            .send(index)
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }

    /// The keymap's layouts, in Ctrl+Space's order: the xkb layout, its
    /// variant and xkb's name for it.
    #[zbus(property)]
    fn layouts(&self) -> Vec<Layout> {
        self.status().layouts
    }

    /// The index of the layout in use.
    #[zbus(property)]
    fn current(&self) -> u32 {
        self.status().current
    }
}

/// On a bus of their own: `dbus-run-session -- cargo test -p
/// patina-compositor keyboard -- --ignored`.
#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use patina_system::keyboard as client;
    use smithay::reexports::calloop::EventLoop;

    use super::*;

    #[test]
    #[ignore = "needs a session bus of its own: run under dbus-run-session"]
    fn clients_read_the_layouts_and_choose_one() {
        let mut event_loop: EventLoop<'static, Vec<u32>> = EventLoop::try_new().unwrap();
        let (send, incoming) = channel::channel::<u32>();
        event_loop
            .handle()
            .insert_source(incoming, |event, _, got| {
                if let channel::Event::Msg(i) = event {
                    got.push(i);
                }
            })
            .unwrap();
        let service = Service::serve(send).expect("the name is free on a bus of our own");
        let layout = |code: &str, variant: &str, name: &str| {
            (code.to_string(), variant.to_string(), name.to_string())
        };
        service.publish(Status {
            layouts: vec![
                layout("us", "", "English (US)"),
                layout("se", "nodeadkeys", "Swedish (no dead keys)"),
            ],
            current: 1,
        });

        let status = client::status().unwrap().expect("served");
        assert_eq!(status.current, 1);
        assert_eq!(status.layouts[1].code, "se");
        assert_eq!(status.layouts[1].variant, "nodeadkeys");
        assert_eq!(status.layouts[0].name, "English (US)");

        client::set_current(0).unwrap();
        assert!(client::set_current(2).is_err(), "there are only two");
        let mut got = Vec::new();
        let deadline = Instant::now() + Duration::from_millis(300);
        while Instant::now() < deadline {
            event_loop
                .dispatch(Some(Duration::from_millis(20)), &mut got)
                .unwrap();
        }
        assert_eq!(got, [0]);
    }
}
