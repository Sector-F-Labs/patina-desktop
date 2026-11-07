//! The monitors, from outside the compositor (ADR-0121): which are
//! connected, whether the desktop extends across them or mirrors, and where
//! each sits, through the compositor's `com.sectorflabs.Patina.Displays` on
//! the session bus. For `patina-displays` (the popup) and Settings.

use std::collections::HashMap;

use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::OwnedValue;

const NAME: &str = "com.sectorflabs.Patina.Displays";
const PATH: &str = "/com/sectorflabs/Patina/Displays";

/// What the desktop does with several monitors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Extend,
    Mirror,
    /// The primary alone: the others are off.
    Off,
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Mode::Extend => "extend",
            Mode::Mirror => "mirror",
            Mode::Off => "off",
        }
    }
}

/// The edge of another monitor one sits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Above,
    Below,
    Left,
    Right,
}

impl Side {
    pub const ALL: [Side; 4] = [Side::Above, Side::Below, Side::Left, Side::Right];

    pub fn name(self) -> &'static str {
        match self {
            Side::Above => "above",
            Side::Below => "below",
            Side::Left => "left",
            Side::Right => "right",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.name() == name)
    }
}

/// A monitor the compositor drives.
#[derive(Debug, Clone, PartialEq)]
pub struct Monitor {
    /// The connector's name: eDP-1, DP-3.
    pub name: String,
    /// Stays when it's plugged in elsewhere.
    pub id: String,
    pub make: String,
    pub model: String,
    /// The mode, in pixels.
    pub width: i32,
    pub height: i32,
    /// Where it is on the desktop, and how large, in points.
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
    /// Where it was put, if it was: on `side` of the monitor with id `to`.
    pub to: String,
    pub side: Option<Side>,
    pub offset: i32,
}

impl Monitor {
    /// Whether it's the machine's own panel, by its connector.
    pub fn internal(&self) -> bool {
        ["eDP", "LVDS", "DSI"]
            .iter()
            .any(|p| self.name.starts_with(p))
    }

    /// What to call it: "Built-in display" for the panel (whose EDID has
    /// only a product code), else its model if it says one, else its
    /// connector.
    pub fn label(&self) -> &str {
        if self.internal() {
            "Built-in display"
        } else if self.model.is_empty() || self.model == "Unknown" {
            &self.name
        } else {
            &self.model
        }
    }

    fn from_dictionary(d: &HashMap<String, OwnedValue>) -> Self {
        let text = |k: &str| {
            d.get(k)
                .and_then(|v| String::try_from(v.try_clone().ok()?).ok())
                .unwrap_or_default()
        };
        let int = |k: &str| d.get(k).and_then(|v| i32::try_from(v).ok()).unwrap_or(0);
        let flag = |k: &str| {
            d.get(k)
                .and_then(|v| bool::try_from(v).ok())
                .unwrap_or(false)
        };
        Self {
            name: text("name"),
            id: text("id"),
            make: text("make"),
            model: text("model"),
            width: int("width"),
            height: int("height"),
            x: int("x"),
            y: int("y"),
            logical_width: int("logical-width"),
            logical_height: int("logical-height"),
            scale: d
                .get("scale")
                .and_then(|v| f64::try_from(v).ok())
                .unwrap_or(1.0),
            primary: flag("primary"),
            known: flag("known"),
            active: flag("active"),
            to: text("to"),
            side: Side::from_name(&text("side")),
            offset: int("offset"),
        }
    }
}

/// A display the compositor remembers (ADR-0124): where it was put, kept
/// whether or not it's connected.
#[derive(Debug, Clone, PartialEq)]
pub struct Saved {
    pub id: String,
    /// What it was called when it was saved.
    pub name: String,
    /// A connected monitor goes by it: its own, or one of its model's.
    pub in_use: bool,
    /// Its monitor is asked about each time it's plugged in; otherwise a
    /// known one is switched on as the desktop was last used (ADR-0126).
    pub ask: bool,
    /// The monitor it's next to, by id and by name; empty for none.
    pub to: String,
    pub to_name: String,
    pub side: Option<Side>,
    pub offset: i32,
}

impl Saved {
    fn from_dictionary(d: &HashMap<String, OwnedValue>) -> Self {
        let text = |k: &str| {
            d.get(k)
                .and_then(|v| String::try_from(v.try_clone().ok()?).ok())
                .unwrap_or_default()
        };
        Self {
            id: text("id"),
            name: text("name"),
            in_use: d
                .get("in-use")
                .and_then(|v| bool::try_from(v).ok())
                .unwrap_or(false),
            ask: d
                .get("ask")
                .and_then(|v| bool::try_from(v).ok())
                .unwrap_or(false),
            to: text("to"),
            to_name: text("to-name"),
            side: Side::from_name(&text("side")),
            offset: d
                .get("offset")
                .and_then(|v| i32::try_from(v).ok())
                .unwrap_or(0),
        }
    }
}

fn proxy(bus: &Connection) -> Result<Proxy<'_>, String> {
    Proxy::new(bus, NAME, PATH, NAME).map_err(|e| e.to_string())
}

fn session() -> Result<Connection, String> {
    Connection::session().map_err(|e| format!("no session bus: {e}"))
}

/// The monitors, the primary first. An error when nothing serves them
/// (nested in another desktop, or under another compositor).
pub fn list() -> Result<Vec<Monitor>, String> {
    let bus = session()?;
    let rows: Vec<HashMap<String, OwnedValue>> =
        proxy(&bus)?.call("List", &()).map_err(|e| e.to_string())?;
    Ok(rows.iter().map(Monitor::from_dictionary).collect())
}

/// The displays the compositor remembers, the one saved last at the end.
pub fn saved() -> Result<Vec<Saved>, String> {
    let bus = session()?;
    let rows: Vec<HashMap<String, OwnedValue>> =
        proxy(&bus)?.call("Saved", &()).map_err(|e| e.to_string())?;
    Ok(rows.iter().map(Saved::from_dictionary).collect())
}

/// Whether the saved display `id` is asked about each time its monitor is
/// plugged in (the popup), or switched on without.
pub fn set_ask(id: &str, ask: bool) -> Result<(), String> {
    let bus = session()?;
    proxy(&bus)?
        .call::<_, _, ()>("SetAsk", &(id, ask))
        .map_err(|e| e.to_string())
}

/// Forgets the saved display `id`: a monitor of that model plugged in later
/// is new again.
pub fn forget(id: &str) -> Result<(), String> {
    let bus = session()?;
    proxy(&bus)?
        .call::<_, _, ()>("Forget", &(id,))
        .map_err(|e| e.to_string())
}

/// Whether the desktop extends or mirrors.
pub fn mode() -> Result<Mode, String> {
    let bus = session()?;
    let mode: String = proxy(&bus)?.call("Mode", &()).map_err(|e| e.to_string())?;
    Ok(match mode.as_str() {
        "mirror" => Mode::Mirror,
        "off" => Mode::Off,
        _ => Mode::Extend,
    })
}

pub fn set_mode(mode: Mode) -> Result<(), String> {
    let bus = session()?;
    proxy(&bus)?
        .call::<_, _, ()>("SetMode", &(mode.name(),))
        .map_err(|e| e.to_string())
}

/// Puts monitor `id` on `side` of monitor `to`, `offset` points along the
/// edge; `None` for after the one before.
pub fn set_place(id: &str, place: Option<(&str, Side, i32)>) -> Result<(), String> {
    let bus = session()?;
    let (to, side, offset) =
        place.map_or(("", "", 0), |(to, side, offset)| (to, side.name(), offset));
    proxy(&bus)?
        .call::<_, _, ()>("SetPlace", &(id, to, side, offset))
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_saved_display_reads_from_its_dictionary() {
        let v = |v: zbus::zvariant::Value<'_>| OwnedValue::try_from(v).unwrap();
        let d = HashMap::from([
            ("id".to_string(), v("DEL-A0D3-HOME1".into())),
            ("name".to_string(), v("DELL U2720Q".into())),
            ("in-use".to_string(), v(false.into())),
            ("ask".to_string(), v(true.into())),
            ("to-name".to_string(), v("Built-in display".into())),
            ("side".to_string(), v("right".into())),
        ]);
        let s = Saved::from_dictionary(&d);
        assert_eq!(
            (s.name.as_str(), s.in_use, s.side),
            ("DELL U2720Q", false, Some(Side::Right))
        );
        assert_eq!(s.to_name, "Built-in display");
        assert!(s.ask);
    }

    #[test]
    fn a_monitor_reads_from_its_dictionary() {
        let v = |v: zbus::zvariant::Value<'_>| OwnedValue::try_from(v).unwrap();
        let d = HashMap::from([
            ("name".to_string(), v("DP-3".into())),
            ("model".to_string(), v("LG HDR WQHD".into())),
            ("width".to_string(), v(3440i32.into())),
            ("scale".to_string(), v(1.25f64.into())),
            ("known".to_string(), v(true.into())),
            ("active".to_string(), v(true.into())),
            ("side".to_string(), v("above".into())),
        ]);
        let m = Monitor::from_dictionary(&d);
        assert_eq!((m.name.as_str(), m.width, m.scale), ("DP-3", 3440, 1.25));
        assert!(m.known && m.active && !m.primary);
        assert_eq!(m.side, Some(Side::Above));
        assert_eq!(m.label(), "LG HDR WQHD");
        let panel = Monitor {
            name: "eDP-1".into(),
            model: "8C44".into(),
            ..m
        };
        assert_eq!(panel.label(), "Built-in display");
    }
}
