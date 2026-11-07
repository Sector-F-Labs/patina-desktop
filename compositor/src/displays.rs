//! Several monitors as the user sees them (ADR-0121): extend the desktop
//! across them or mirror it, and where each monitor sits, remembered by
//! monitor so it comes back wherever it's plugged in. The service on the bus
//! (`displays/dbus.rs`) and the popup (`patina-displays`) both work through
//! this; Settings' Displays page edits the places.
//!
//! The choice is kept in `displays.toml` beside the compositor's config
//! (which the compositor never writes), so a monitor in it is one seen
//! before, and one that isn't is new.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use smithay::output::Output;
use smithay::reexports::calloop::timer::{TimeoutAction, Timer};

use crate::layout::{Place, Side};
use crate::state::State;

pub mod dbus;

/// What the desktop does with several monitors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// Each monitor shows its own part of one large desktop.
    #[default]
    Extend,
    /// Every monitor shows the primary's part of it.
    Mirror,
    /// Only the primary is driven; the others stay off.
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

    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "extend" => Some(Mode::Extend),
            "mirror" => Some(Mode::Mirror),
            "off" => Some(Mode::Off),
            _ => None,
        }
    }
}

impl Side {
    pub fn name(self) -> &'static str {
        match self {
            Side::Above => "above",
            Side::Below => "below",
            Side::LeftOf => "left",
            Side::RightOf => "right",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "above" => Some(Side::Above),
            "below" => Some(Side::Below),
            "left" => Some(Side::LeftOf),
            "right" => Some(Side::RightOf),
            _ => None,
        }
    }
}

/// A monitor's identity (`edid::Edid::id`, else its connector's name), kept
/// in its output.
#[derive(Debug, Clone)]
pub struct MonitorId(pub String);

/// The scale its density alone gives, to go back to when it's no longer
/// mirroring.
#[derive(Debug, Clone, Copy)]
pub struct AutoScale(pub f64);

/// `output`'s identity.
pub fn id_of(output: &Output) -> String {
    output
        .user_data()
        .get::<MonitorId>()
        .map_or_else(|| output.name(), |id| id.0.clone())
}

/// The scale `output`'s density gives.
pub fn auto_scale_of(output: &Output) -> f64 {
    output
        .user_data()
        .get::<AutoScale>()
        .map_or(1.0, |scale| scale.0)
}

/// One monitor in `displays.toml`: where it goes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Saved {
    pub id: String,
    /// The monitor it's next to, by id; empty for the primary or when it
    /// goes after the one before.
    #[serde(default)]
    pub to: String,
    #[serde(default)]
    pub side: String,
    #[serde(default)]
    pub offset: i32,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct File {
    #[serde(default)]
    mode: String,
    #[serde(default)]
    monitor: Vec<Saved>,
}

#[derive(Default)]
pub struct Displays {
    pub mode: Mode,
    saved: Vec<Saved>,
    path: Option<PathBuf>,
    pub service: Option<dbus::Service>,
}

fn path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("patina-compositor").join("displays.toml"))
}

impl Displays {
    /// What was kept, or nothing yet.
    pub fn load() -> Self {
        let path = path();
        let file = path
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map(|text| {
                toml::from_str::<File>(&text).unwrap_or_else(|e| {
                    eprintln!("patina-compositor: displays.toml: {e}");
                    File::default()
                })
            })
            .unwrap_or_default();
        Self {
            mode: Mode::from_name(&file.mode).unwrap_or_default(),
            saved: file.monitor,
            path,
            service: None,
        }
    }

    fn file(&self) -> String {
        let file = File {
            mode: self.mode.name().into(),
            monitor: self.saved.clone(),
        };
        toml::to_string(&file).unwrap_or_default()
    }

    /// Writes what's kept, best effort.
    pub fn save(&self) {
        let Some(path) = &self.path else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Err(e) = std::fs::write(path, self.file()) {
            eprintln!("patina-compositor: can't save {}: {e}", path.display());
        }
    }

    /// Whether this monitor was placed before.
    pub fn known(&self, id: &str) -> bool {
        self.saved.iter().any(|s| s.id == id)
    }

    /// Where `id` was put, if it was and it's next to some monitor.
    pub fn place(&self, id: &str) -> Option<Place> {
        let saved = self.saved.iter().find(|s| s.id == id)?;
        Some(Place {
            to: saved.to.clone(),
            side: Side::from_name(&saved.side)?,
            offset: saved.offset,
        })
    }

    /// Remembers `id` as seen, and put at `place` (or after the one before).
    pub fn set(&mut self, id: &str, place: Option<Place>) {
        let saved = Saved {
            id: id.into(),
            to: place.as_ref().map(|p| p.to.clone()).unwrap_or_default(),
            side: place.as_ref().map(|p| p.side.name().into()).unwrap_or_default(),
            offset: place.map_or(0, |p| p.offset),
        };
        match self.saved.iter_mut().find(|s| s.id == id) {
            Some(existing) => *existing = saved,
            None => self.saved.push(saved),
        }
    }
}

impl State {
    /// Starts the service on the bus, once.
    pub fn start_displays(&mut self) {
        match dbus::Service::start(&self.loop_handle) {
            Ok(service) => self.displays.service = Some(service),
            Err(e) => eprintln!("patina-compositor: the monitors aren't on D-Bus: {e}"),
        }
    }

    /// The monitors, the primary first.
    pub fn monitors(&self) -> Vec<dbus::Monitor> {
        self.outputs
            .iter()
            .map(|o| {
                let id = id_of(o);
                let area = self.space.output_geometry(o).unwrap_or_default();
                let place = self.displays.place(&id);
                let props = o.physical_properties();
                let size = o.current_mode().map(|m| m.size).unwrap_or_default();
                dbus::Monitor {
                    name: o.name(),
                    known: self.displays.known(&id),
                    active: true,
                    to: place.as_ref().map(|p| p.to.clone()).unwrap_or_default(),
                    side: place.as_ref().map(|p| p.side.name().into()).unwrap_or_default(),
                    offset: place.map_or(0, |p| p.offset),
                    id,
                    make: props.make,
                    model: props.model,
                    width: size.w,
                    height: size.h,
                    x: area.loc.x,
                    y: area.loc.y,
                    logical_width: area.size.w,
                    logical_height: area.size.h,
                    scale: o.current_scale().fractional_scale(),
                    primary: *o == self.output,
                }
            })
            .collect()
    }

    /// The monitors connected but not driven, listed after the others.
    fn idle_monitors(&self) -> Vec<dbus::Monitor> {
        let Some(u) = self.udev.as_ref() else {
            return Vec::new();
        };
        u.idle_monitors()
            .into_iter()
            .map(|m| dbus::Monitor {
                known: self.displays.known(&m.id),
                name: m.name,
                id: m.id,
                make: m.make,
                model: m.model,
                width: m.width,
                height: m.height,
                x: 0,
                y: 0,
                logical_width: 0,
                logical_height: 0,
                scale: 1.0,
                primary: false,
                active: false,
                to: String::new(),
                side: String::new(),
                offset: 0,
            })
            .collect()
    }

    /// A request from the bus.
    pub fn displays_request(&mut self, request: dbus::Request) {
        use dbus::Request;
        match request {
            Request::List(reply) => {
                let mut all = self.monitors();
                all.extend(self.idle_monitors());
                let _ = reply.send(all);
            }
            Request::Mode(reply) => {
                let _ = reply.send(self.displays.mode.name().into());
            }
            Request::SetMode(mode, reply) => {
                let _ = reply.send(match Mode::from_name(&mode) {
                    Some(mode) => {
                        self.set_display_mode(mode);
                        Ok(())
                    }
                    None => Err(format!("{mode:?}: the mode is extend, mirror or off")),
                });
            }
            Request::SetPlace {
                id,
                to,
                side,
                offset,
                reply,
            } => {
                let _ = reply.send(self.set_monitor_place(&id, &to, &side, offset));
            }
        }
    }

    /// Extends the desktop across the monitors, mirrors it on them, or
    /// drives the primary alone. The monitors waiting are switched on for
    /// the first two; those driven are switched off for the third.
    pub fn set_display_mode(&mut self, mode: Mode) {
        if self.displays.mode != mode {
            self.displays.mode = mode;
            self.displays.save();
        }
        let config = self.config.clone();
        let Some(u) = self.udev.as_mut() else {
            self.monitors_changed();
            return;
        };
        match mode {
            Mode::Off => {
                let removed = u.deactivate_secondaries();
                if !removed.is_empty() {
                    let all = u.outputs();
                    self.outputs_changed(all, &removed);
                }
            }
            Mode::Extend | Mode::Mirror => {
                if !u.activate_idle(&config).is_empty() {
                    let all = u.outputs();
                    self.outputs_changed(all, &[]);
                }
            }
        }
        self.monitors_changed();
    }

    /// Puts monitor `id` on `side` of monitor `to`; an empty `side` puts it
    /// after the one before. Either way it is known from now on.
    pub fn set_monitor_place(
        &mut self,
        id: &str,
        to: &str,
        side: &str,
        offset: i32,
    ) -> Result<(), String> {
        if !self.outputs.iter().any(|o| id_of(o) == id) {
            return Err(format!("no monitor {id}"));
        }
        let place = if side.is_empty() {
            None
        } else {
            let side = Side::from_name(side)
                .ok_or_else(|| format!("{side:?}: the side is above, below, left or right"))?;
            if to == id || !self.outputs.iter().any(|o| id_of(o) == to) {
                return Err(format!("no other monitor {to}"));
            }
            Some(Place {
                to: to.into(),
                side,
                offset,
            })
        };
        self.displays.set(id, place);
        self.displays.save();
        self.monitors_changed();
        Ok(())
    }

    /// Lays the monitors out again and says so on the bus.
    fn monitors_changed(&mut self) {
        self.outputs_changed(self.outputs.clone(), &[]);
        if let Some(service) = &self.displays.service {
            service.changed();
        }
    }

    /// Monitors were plugged in (each an id and a connector name) and left
    /// off: the popup asks what to do, and the bus says which, and whether
    /// they're known.
    pub fn monitors_plugged(&mut self, added: &[(String, String)]) {
        // A monitor to choose for: the popup asks (not over a lock). A
        // moment later: bringing the monitor up keeps the loop busy, and the
        // popup's first question to it would time out.
        if !added.is_empty() {
            let _ = self.loop_handle.insert_source(
                Timer::from_duration(std::time::Duration::from_millis(1500)),
                |_, _, state| {
                    if !state.locked() {
                        state.spawn_client("patina-displays");
                    }
                    TimeoutAction::Drop
                },
            );
        }
        if let Some(service) = &self.displays.service {
            service.changed();
            for (id, name) in added {
                service.added(id, name, self.displays.known(id));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_monitor_is_new_until_it_is_set() {
        let mut d = Displays::default();
        assert!(!d.known("GSM-5BBF-1"));
        assert_eq!(d.place("GSM-5BBF-1"), None);
        d.set(
            "GSM-5BBF-1",
            Some(Place {
                to: "BOE-0A1B-2".into(),
                side: Side::Above,
                offset: 40,
            }),
        );
        assert!(d.known("GSM-5BBF-1"));
        let place = d.place("GSM-5BBF-1").unwrap();
        assert_eq!((place.side, place.offset), (Side::Above, 40));
        // Seen, but with nowhere in particular: after the one before.
        d.set("X-1-1", None);
        assert!(d.known("X-1-1"));
        assert_eq!(d.place("X-1-1"), None);
    }

    #[test]
    fn what_is_kept_reads_back() {
        let mut d = Displays {
            mode: Mode::Mirror,
            ..Displays::default()
        };
        d.set(
            "GSM-5BBF-1",
            Some(Place {
                to: "a".into(),
                side: Side::LeftOf,
                offset: -8,
            }),
        );
        let text = d.file();
        let back: File = toml::from_str(&text).unwrap();
        assert_eq!(Mode::from_name(&back.mode), Some(Mode::Mirror));
        assert_eq!(back.monitor, d.saved);
    }

    #[test]
    fn names_round_trip() {
        for side in [Side::Above, Side::Below, Side::LeftOf, Side::RightOf] {
            assert_eq!(Side::from_name(side.name()), Some(side));
        }
        assert_eq!(Mode::from_name("mirror"), Some(Mode::Mirror));
        assert_eq!(Mode::from_name("both"), None);
    }
}
