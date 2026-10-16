//! Power modes (ADR-0042): power-saver, balanced and performance, through
//! power-profiles-daemon on the system bus, as GNOME's settings have them.
//! The daemon knows what the hardware's driver can do; Patina only asks it.

use std::collections::HashMap;

use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::OwnedValue;

/// The daemon's current name, then the one before 0.20.
const NAMES: [(&str, &str); 2] = [
    (
        "org.freedesktop.UPower.PowerProfiles",
        "/org/freedesktop/UPower/PowerProfiles",
    ),
    ("net.hadess.PowerProfiles", "/net/hadess/PowerProfiles"),
];

/// An app holding a mode, such as a game asking for performance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hold {
    pub app: String,
    pub profile: String,
    pub reason: String,
}

/// How things stand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    /// The modes this machine has, as the daemon names them, in its order.
    pub profiles: Vec<String>,
    pub active: String,
    /// Why performance is held back ("lap-detected",
    /// "high-operating-temperature"), or empty.
    pub degraded: String,
    pub holds: Vec<Hold>,
}

fn proxy(bus: &Connection) -> Result<Option<Proxy<'static>>, String> {
    let dbus = zbus::blocking::fdo::DBusProxy::new(bus).map_err(|e| e.to_string())?;
    for (name, path) in NAMES {
        let bus_name = zbus::names::BusName::try_from(name).map_err(|e| e.to_string())?;
        // Started on demand by D-Bus activation, so either running or
        // activatable counts.
        let here = dbus.name_has_owner(bus_name.clone()).unwrap_or(false)
            || dbus
                .list_activatable_names()
                .is_ok_and(|names| names.iter().any(|n| n.as_str() == name));
        if here {
            return Proxy::new(bus, name, path, name)
                .map(Some)
                .map_err(|e| e.to_string());
        }
    }
    Ok(None)
}

fn text(map: &HashMap<String, OwnedValue>, key: &str) -> String {
    map.get(key)
        .and_then(|v| String::try_from(v.clone()).ok())
        .unwrap_or_default()
}

/// The daemon's view; `None` where it isn't installed (FreeBSD, or a
/// desktop without it).
pub fn status() -> Result<Option<Status>, String> {
    let bus = Connection::system().map_err(|e| format!("no system bus: {e}"))?;
    let Some(p) = proxy(&bus)? else {
        return Ok(None);
    };
    let get = |e: zbus::Error| e.to_string();
    let profiles: Vec<HashMap<String, OwnedValue>> = p.get_property("Profiles").map_err(get)?;
    let holds: Vec<HashMap<String, OwnedValue>> =
        p.get_property("ActiveProfileHolds").map_err(get)?;
    Ok(Some(Status {
        profiles: profiles.iter().map(|m| text(m, "Profile")).collect(),
        active: p.get_property("ActiveProfile").map_err(get)?,
        degraded: p.get_property("PerformanceDegraded").unwrap_or_default(),
        holds: holds
            .iter()
            .map(|m| Hold {
                app: text(m, "ApplicationId"),
                profile: text(m, "Profile"),
                reason: text(m, "Reason"),
            })
            .collect(),
    }))
}

/// Switches to `profile`.
pub fn set(profile: &str) -> Result<(), String> {
    let bus = Connection::system().map_err(|e| format!("no system bus: {e}"))?;
    let p = proxy(&bus)?.ok_or("power-profiles-daemon isn't installed")?;
    p.set_property("ActiveProfile", profile)
        .map_err(|e| e.to_string())
}

/// A mode's name to show, and a line about it, as GNOME words them.
pub fn describe(profile: &str) -> (&str, &str) {
    match profile {
        "performance" => ("Performance", "High performance and power usage"),
        "balanced" => ("Balanced", "Standard performance and power usage"),
        "power-saver" => ("Power Saver", "Reduced performance and power usage"),
        other => (other, ""),
    }
}

/// Why performance is held back, in words.
pub fn degraded_reason(reason: &str) -> String {
    match reason {
        "lap-detected" => "Performance is limited while the computer is on a lap.".into(),
        "high-operating-temperature" => {
            "Performance is limited because the computer is hot.".into()
        }
        "" => String::new(),
        other => format!("Performance is limited ({other})."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_and_reasons_in_words() {
        assert_eq!(describe("power-saver").0, "Power Saver");
        assert_eq!(describe("custom").0, "custom");
        assert!(degraded_reason("lap-detected").contains("lap"));
        assert!(degraded_reason("").is_empty());
        assert!(degraded_reason("odd").contains("odd"));
    }
}
