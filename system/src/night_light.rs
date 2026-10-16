//! Night Light, from outside the compositor (ADR-0039): its state, an
//! override, and inhibitors, through the compositor's
//! `com.sectorflabs.Patina.NightLight` on the session bus. For Settings and
//! `patina-night-light`; an app wanting true colour holds an [`Inhibitor`].

use zbus::blocking::{Connection, Proxy};

const NAME: &str = "com.sectorflabs.Patina.NightLight";
const PATH: &str = "/com/sectorflabs/Patina/NightLight";

/// What the compositor says about Night Light.
#[derive(Debug, Clone, PartialEq)]
pub struct Status {
    /// The monitor has a gamma ramp to warm.
    pub available: bool,
    /// The screen is warm, or warming.
    pub active: bool,
    /// The reasons given by those holding it off.
    pub inhibitors: Vec<String>,
    /// The temperature on screen, in kelvin.
    pub temperature: u32,
    /// "auto", "on" or "off".
    pub overridden: String,
}

fn proxy(bus: &Connection) -> Result<Proxy<'_>, String> {
    Proxy::new(bus, NAME, PATH, NAME).map_err(|e| e.to_string())
}

fn session() -> Result<Connection, String> {
    Connection::session().map_err(|e| format!("no session bus: {e}"))
}

/// Whether the compositor serves Night Light here: not when it runs nested
/// in another desktop, nor under another compositor.
fn served(bus: &Connection) -> Result<bool, String> {
    let dbus = zbus::blocking::fdo::DBusProxy::new(bus).map_err(|e| e.to_string())?;
    let name = zbus::names::BusName::try_from(NAME).map_err(|e| e.to_string())?;
    dbus.name_has_owner(name).map_err(|e| e.to_string())
}

/// Night Light's state; `None` when nothing serves it (nested, or another
/// desktop).
pub fn status() -> Result<Option<Status>, String> {
    let bus = session()?;
    if !served(&bus)? {
        return Ok(None);
    }
    let p = proxy(&bus)?;
    let get = |e: zbus::Error| e.to_string();
    Ok(Some(Status {
        available: p.get_property("Available").map_err(get)?,
        active: p.get_property("Active").map_err(get)?,
        inhibitors: p.get_property("Inhibitors").map_err(get)?,
        temperature: p.get_property("Temperature").map_err(get)?,
        overridden: p.get_property("Override").map_err(get)?,
    }))
}

/// "on" or "off" until the schedule next changes, or "auto" to follow it.
pub fn set_override(state: &str) -> Result<(), String> {
    let bus = session()?;
    proxy(&bus)?
        .call::<_, _, ()>("SetOverride", &(state,))
        .map_err(|e| e.to_string())
}

/// Night Light held off while this lives, or until the process leaves the
/// bus.
pub struct Inhibitor {
    bus: Connection,
    cookie: u32,
}

/// Holds Night Light off, saying why (shown in Settings).
pub fn inhibit(reason: &str) -> Result<Inhibitor, String> {
    let bus = session()?;
    let cookie = proxy(&bus)?
        .call("Inhibit", &(reason,))
        .map_err(|e| e.to_string())?;
    Ok(Inhibitor { bus, cookie })
}

impl Drop for Inhibitor {
    fn drop(&mut self) {
        if let Ok(p) = proxy(&self.bus) {
            let _ = p.call::<_, _, ()>("Uninhibit", &(self.cookie,));
        }
    }
}
