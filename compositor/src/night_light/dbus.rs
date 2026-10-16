//! Night Light on the session bus (ADR-0039), for Settings,
//! `patina-night-light` and apps that need true colour:
//! `com.sectorflabs.Patina.NightLight` at `/com/sectorflabs/Patina/NightLight`.
//!
//! zbus runs on threads of its own. Requests reach the event loop over a
//! calloop channel; what the compositor decides comes back as a `Status`,
//! which the interface's properties read and announce.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, mpsc};

use smithay::reexports::calloop::{LoopHandle, channel};
use zbus::message::Header;

use super::Override;
use crate::state::State;

pub const NAME: &str = "com.sectorflabs.Patina.NightLight";
pub const PATH: &str = "/com/sectorflabs/Patina/NightLight";

/// What the bus asks of the compositor.
#[derive(Debug)]
pub enum Request {
    Inhibit {
        cookie: u32,
        owner: String,
        reason: String,
    },
    Uninhibit {
        cookie: u32,
        owner: String,
    },
    /// A name left the bus; its inhibitors go with it.
    Gone(String),
    Override(Override),
}

/// What the properties say.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Status {
    pub available: bool,
    pub active: bool,
    /// Why each inhibitor holds it off.
    pub inhibitors: Vec<String>,
    pub temperature: u32,
    pub overridden: String,
}

/// The compositor's end: where to send what it decides.
pub struct Service {
    status: Arc<Mutex<Status>>,
    changed: mpsc::Sender<()>,
}

impl Service {
    /// Claims the name and serves the interface, on threads of its own.
    /// Requests come into the loop at `handle`.
    pub fn start(handle: &LoopHandle<'static, State>) -> Result<Self, String> {
        let (requests, incoming) = channel::channel::<Request>();
        handle
            .insert_source(incoming, |event, _, state| {
                if let channel::Event::Msg(request) = event {
                    state.night_light_request(request);
                }
            })
            .map_err(|e| e.to_string())?;
        Self::serve(requests)
    }

    /// Claims the name and serves the interface, sending what's asked to
    /// `requests`.
    fn serve(requests: channel::Sender<Request>) -> Result<Self, String> {
        let status = Arc::new(Mutex::new(Status::default()));
        let (changed, announce) = mpsc::channel();
        let interface = Interface {
            requests: requests.clone(),
            status: status.clone(),
            next: AtomicU32::new(1),
        };
        let connection = zbus::blocking::connection::Builder::session()
            .and_then(|b| b.name(NAME))
            .and_then(|b| b.serve_at(PATH, interface))
            .and_then(|b| b.build())
            .map_err(|e| e.to_string())?;
        let watched = connection.clone();
        std::thread::Builder::new()
            .name("night-light-owners".into())
            .spawn(move || watch_owners(&watched, &requests))
            .map_err(|e| e.to_string())?;
        std::thread::Builder::new()
            .name("night-light".into())
            .spawn(move || announce_changes(&connection, &announce))
            .map_err(|e| e.to_string())?;
        Ok(Self { status, changed })
    }

    /// The compositor's latest; announced if anything but the temperature
    /// changed, or `settled` (a fade ended) and the temperature did.
    pub fn publish(&self, new: Status, settled: bool) {
        let Ok(mut status) = self.status.lock() else {
            return;
        };
        let temperature_only = Status {
            temperature: status.temperature,
            ..new.clone()
        } == *status;
        let worth = !temperature_only || (settled && new.temperature != status.temperature);
        *status = new;
        drop(status);
        if worth {
            let _ = self.changed.send(());
        }
    }
}

/// Drops the inhibitors of names that leave the bus.
fn watch_owners(connection: &zbus::blocking::Connection, requests: &channel::Sender<Request>) {
    let owners = zbus::blocking::fdo::DBusProxy::new(connection)
        .and_then(|bus| bus.receive_name_owner_changed());
    let owners = match owners {
        Ok(o) => o,
        Err(e) => {
            eprintln!("patina-compositor: night light can't watch the bus: {e}");
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

/// Emits PropertiesChanged when the compositor says something changed.
fn announce_changes(connection: &zbus::blocking::Connection, changed: &mpsc::Receiver<()>) {
    let Ok(interface) = connection.object_server().interface::<_, Interface>(PATH) else {
        return;
    };
    while changed.recv().is_ok() {
        let emitter = interface.signal_emitter();
        let i = interface.get();
        let result = zbus::block_on(async {
            i.active_changed(emitter).await?;
            i.available_changed(emitter).await?;
            i.inhibitors_changed(emitter).await?;
            i.temperature_changed(emitter).await?;
            i.override_changed(emitter).await
        });
        if let Err(e) = result {
            eprintln!("patina-compositor: night light: {e}");
        }
    }
}

struct Interface {
    requests: channel::Sender<Request>,
    status: Arc<Mutex<Status>>,
    next: AtomicU32,
}

impl Interface {
    fn status(&self) -> Status {
        self.status.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

fn sender(header: &Header<'_>) -> zbus::fdo::Result<String> {
    header
        .sender()
        .map(|s| s.to_string())
        .ok_or_else(|| zbus::fdo::Error::Failed("no sender".into()))
}

#[zbus::interface(name = "com.sectorflabs.Patina.NightLight")]
impl Interface {
    /// Holds Night Light off until `Uninhibit` with the cookie, or until
    /// the caller leaves the bus.
    fn inhibit(
        &self,
        reason: String,
        #[zbus(header)] header: Header<'_>,
    ) -> zbus::fdo::Result<u32> {
        let owner = sender(&header)?;
        let cookie = self.next.fetch_add(1, Ordering::Relaxed);
        self.requests
            .send(Request::Inhibit {
                cookie,
                owner,
                reason,
            })
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
        Ok(cookie)
    }

    fn uninhibit(&self, cookie: u32, #[zbus(header)] header: Header<'_>) -> zbus::fdo::Result<()> {
        let owner = sender(&header)?;
        self.requests
            .send(Request::Uninhibit { cookie, owner })
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }

    /// "on" or "off" until the schedule next changes, or "auto" to follow
    /// it again.
    fn set_override(&self, state: String) -> zbus::fdo::Result<()> {
        let o = Override::from_name(&state).ok_or_else(|| {
            zbus::fdo::Error::InvalidArgs(format!("{state:?}: expected auto, on or off"))
        })?;
        self.requests
            .send(Request::Override(o))
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }

    /// The screen is warm, or warming.
    #[zbus(property)]
    fn active(&self) -> bool {
        self.status().active
    }

    /// The monitor has a gamma ramp to set.
    #[zbus(property)]
    fn available(&self) -> bool {
        self.status().available
    }

    /// The reasons given by those holding it off.
    #[zbus(property)]
    fn inhibitors(&self) -> Vec<String> {
        self.status().inhibitors
    }

    /// The temperature on screen, in kelvin.
    #[zbus(property)]
    fn temperature(&self) -> u32 {
        self.status().temperature
    }

    /// "auto", "on" or "off".
    #[zbus(property, name = "Override")]
    fn override_state(&self) -> String {
        self.status().overridden
    }
}

/// On a bus of their own: `dbus-run-session -- cargo test -p
/// patina-compositor night_light -- --ignored`.
#[cfg(test)]
mod tests {
    use std::time::Duration;

    use patina_system::night_light as client;
    use smithay::reexports::calloop::EventLoop;

    use super::*;

    /// The requests that reach the loop within `wait`.
    fn requests(event_loop: &mut EventLoop<'static, Vec<Request>>, wait: Duration) -> Vec<Request> {
        let mut got = Vec::new();
        let deadline = std::time::Instant::now() + wait;
        while std::time::Instant::now() < deadline {
            event_loop
                .dispatch(Some(Duration::from_millis(20)), &mut got)
                .unwrap();
        }
        got
    }

    #[test]
    #[ignore = "needs a session bus of its own: run under dbus-run-session"]
    fn clients_read_status_inhibit_and_override() {
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
        service.publish(
            Status {
                available: true,
                active: true,
                inhibitors: vec!["Photograph".into()],
                temperature: 4000,
                overridden: "auto".into(),
            },
            true,
        );

        let status = client::status().unwrap().expect("served");
        assert!(status.available && status.active);
        assert_eq!(status.temperature, 4000);
        assert_eq!(status.inhibitors, ["Photograph"]);
        assert_eq!(status.overridden, "auto");

        // The status call's connection has left the bus by now.
        requests(&mut event_loop, Duration::from_millis(300));
        let held = client::inhibit("editing photos").unwrap();
        let got = requests(&mut event_loop, Duration::from_millis(300));
        let [
            Request::Inhibit {
                cookie,
                owner,
                reason,
            },
        ] = &got[..]
        else {
            panic!("{got:?}");
        };
        assert_eq!(reason, "editing photos");
        let (cookie, owner) = (*cookie, owner.clone());
        drop(held);
        // Letting go: Uninhibit, then the client's connection leaves the bus.
        let got = requests(&mut event_loop, Duration::from_millis(300));
        assert!(
            matches!(&got[..], [Request::Uninhibit { cookie: c, owner: o }, Request::Gone(g)]
                if *c == cookie && *o == owner && *g == owner),
            "{got:?}"
        );

        client::set_override("off").unwrap();
        assert!(client::set_override("sometimes").is_err());
        let got = requests(&mut event_loop, Duration::from_millis(300));
        assert!(
            got.iter()
                .any(|r| matches!(r, Request::Override(Override::Off))),
            "{got:?}"
        );
    }
}
