//! Ending the session and the machine (ADR-0048): locking and logging out
//! through the compositor's `com.sectorflabs.Patina.Session` on the session
//! bus, and restarting and shutting down through the login manager on the
//! system bus: logind (`org.freedesktop.login1`) where there's systemd, and
//! ConsoleKit2, its look-alike, elsewhere (FreeBSD). For the bar's power
//! menu.

use zbus::blocking::{Connection, Proxy};

const SESSION: &str = "com.sectorflabs.Patina.Session";
const SESSION_PATH: &str = "/com/sectorflabs/Patina/Session";

/// A login manager: its bus name, object and interface.
struct Manager {
    name: &'static str,
    path: &'static str,
    interface: &'static str,
}

const LOGIND: Manager = Manager {
    name: "org.freedesktop.login1",
    path: "/org/freedesktop/login1",
    interface: "org.freedesktop.login1.Manager",
};

const CONSOLEKIT: Manager = Manager {
    name: "org.freedesktop.ConsoleKit",
    path: "/org/freedesktop/ConsoleKit/Manager",
    interface: "org.freedesktop.ConsoleKit.Manager",
};

/// Restarting or shutting down the machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Power {
    Restart,
    ShutDown,
}

impl Power {
    /// The login manager's method, and the one asking whether it may.
    fn methods(self) -> (&'static str, &'static str) {
        match self {
            Power::Restart => ("Reboot", "CanReboot"),
            Power::ShutDown => ("PowerOff", "CanPowerOff"),
        }
    }
}

/// What the power menu can do here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Abilities {
    /// Patina's compositor is serving the session: Lock and Log Out.
    pub session: bool,
    pub restart: bool,
    pub shut_down: bool,
}

fn has_owner(bus: &Connection, name: &str) -> bool {
    let Ok(dbus) = zbus::blocking::fdo::DBusProxy::new(bus) else {
        return false;
    };
    zbus::names::BusName::try_from(name)
        .ok()
        .and_then(|n| dbus.name_has_owner(n).ok())
        .unwrap_or(false)
}

fn manager_proxy(bus: &Connection, m: &Manager) -> Option<Proxy<'static>> {
    // ConsoleKit is started on demand, so it may have no owner yet.
    Proxy::new(bus, m.name, m.path, m.interface).ok()
}

/// Whether the login manager would let this user do `power` now: "yes", or
/// "challenge" (it asks for a password through polkit).
fn can(bus: &Connection, power: Power) -> bool {
    let (_, ask) = power.methods();
    [LOGIND, CONSOLEKIT].iter().any(|m| {
        manager_proxy(bus, m)
            .and_then(|p| p.call::<_, _, String>(ask, &()).ok())
            .is_some_and(|answer| answer == "yes" || answer == "challenge")
    })
}

/// What can be done here, asked of the session and system buses.
pub fn abilities() -> Abilities {
    let session = Connection::session()
        .map(|bus| has_owner(&bus, SESSION))
        .unwrap_or(false);
    let (restart, shut_down) = match Connection::system() {
        Ok(bus) => (can(&bus, Power::Restart), can(&bus, Power::ShutDown)),
        Err(_) => (false, false),
    };
    Abilities {
        session,
        restart,
        shut_down,
    }
}

fn compositor(method: &str) -> Result<(), String> {
    let bus = Connection::session().map_err(|e| format!("no session bus: {e}"))?;
    Proxy::new(&bus, SESSION, SESSION_PATH, SESSION)
        .and_then(|p| p.call::<_, _, ()>(method, &()))
        .map_err(|e| e.to_string())
}

/// Locks the screen with the compositor's lock screen.
pub fn lock() -> Result<(), String> {
    compositor("Lock")
}

/// Ends the session: the compositor quits, and with it every app.
pub fn log_out() -> Result<(), String> {
    compositor("LogOut")
}

/// Restarts or shuts down the machine through the login manager, which
/// may ask for a password.
pub fn power(power: Power) -> Result<(), String> {
    let bus = Connection::system().map_err(|e| format!("no system bus: {e}"))?;
    let (method, _) = power.methods();
    let mut last = String::from("no login manager (logind or ConsoleKit2)");
    for m in [LOGIND, CONSOLEKIT] {
        let Some(proxy) = manager_proxy(&bus, &m) else {
            continue;
        };
        // true: interactive, so polkit may ask for a password.
        match proxy.call::<_, _, ()>(method, &(true,)) {
            Ok(()) => return Ok(()),
            Err(e) => last = e.to_string(),
        }
    }
    Err(last)
}
