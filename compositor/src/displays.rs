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

/// A monitor's model: its id without the serial ("GSM-772E" for
/// "GSM-772E-0005F79F"), which every monitor of the model has the same. An
/// id that isn't maker, product and serial (a connector's name, for a
/// monitor that gave no EDID) is its own model.
pub fn model_key(id: &str) -> &str {
    let mut parts = id.splitn(3, '-');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(make), Some(product), Some(_))
            if make.len() == 3
                && make.bytes().all(|b| b.is_ascii_uppercase())
                && product.len() == 4
                && product.bytes().all(|b| b.is_ascii_hexdigit()) =>
        {
            &id[..8]
        }
        _ => id,
    }
}

/// What to call `output`: "Built-in display" for the machine's own panel,
/// else its model, else its connector.
pub fn label_of(output: &Output) -> String {
    let name = output.name();
    let model = output.physical_properties().model;
    if ["eDP", "LVDS", "DSI"].iter().any(|p| name.starts_with(p)) {
        "Built-in display".into()
    } else if model.is_empty() || model == "Unknown" {
        name
    } else {
        model
    }
}

/// What to call a listed monitor: as [`label_of`] says for its output.
fn monitor_label(m: &dbus::Monitor) -> String {
    if ["eDP", "LVDS", "DSI"].iter().any(|p| m.name.starts_with(p)) {
        "Built-in display".into()
    } else if m.model.is_empty() || m.model == "Unknown" {
        m.name.clone()
    } else {
        m.model.clone()
    }
}

/// One monitor in `displays.toml`: where it goes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Saved {
    pub id: String,
    /// What it was called when it was saved, to show it when it isn't
    /// connected.
    #[serde(default)]
    pub name: String,
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

    /// For each of `ids` (the monitors connected, driven or not), the saved
    /// entry it goes by (ADR-0124). A monitor goes by its own entry, kept
    /// by its id (maker, model and serial). One with none goes by an entry
    /// of the same model, the one saved last first, each entry by one
    /// monitor at most: a hot desk's Dells go where a Dell was put, and
    /// two of them take the two places two were given.
    pub fn resolve(&self, ids: &[String]) -> Vec<Option<usize>> {
        let mut claimed = vec![false; self.saved.len()];
        let mut by = vec![None; ids.len()];
        for (i, id) in ids.iter().enumerate() {
            if let Some(e) = self.saved.iter().position(|s| s.id == *id) {
                claimed[e] = true;
                by[i] = Some(e);
            }
        }
        for (i, id) in ids.iter().enumerate() {
            if by[i].is_some() {
                continue;
            }
            let model = model_key(id);
            let like = (0..self.saved.len())
                .rev()
                .find(|&e| !claimed[e] && model_key(&self.saved[e].id) == model);
            if let Some(e) = like {
                claimed[e] = true;
                by[i] = Some(e);
            }
        }
        by
    }

    /// Where each of `ids` goes, as the entry it goes by says, with the
    /// monitor it's next to being the connected one that entry's monitor
    /// is: the same id, or the one that goes by that monitor's entry. A
    /// monitor with no entry, or whose neighbour isn't connected, has no
    /// place (after the one before).
    pub fn places(&self, ids: &[String]) -> Vec<Option<Place>> {
        let by = self.resolve(ids);
        (0..ids.len())
            .map(|i| {
                let saved = &self.saved[by[i]?];
                let side = Side::from_name(&saved.side)?;
                let anchor = ids.iter().position(|id| *id == saved.to).or_else(|| {
                    let entry = self.saved.iter().position(|s| s.id == saved.to)?;
                    by.iter().position(|b| *b == Some(entry))
                })?;
                (anchor != i).then(|| Place {
                    to: ids[anchor].clone(),
                    side,
                    offset: saved.offset,
                })
            })
            .collect()
    }

    /// Remembers `id`, called `name`, as put at `place` (or after the one
    /// before). It becomes the last saved, so a monitor of its model that
    /// isn't known goes where it was put before any other of the model.
    pub fn set(&mut self, id: &str, name: &str, place: Option<Place>) {
        self.saved.retain(|s| s.id != id);
        self.saved.push(Saved {
            id: id.into(),
            name: name.into(),
            to: place.as_ref().map(|p| p.to.clone()).unwrap_or_default(),
            side: place
                .as_ref()
                .map(|p| p.side.name().into())
                .unwrap_or_default(),
            offset: place.map_or(0, |p| p.offset),
        });
    }

    /// Forgets the entry `id`; whether there was one.
    pub fn forget(&mut self, id: &str) -> bool {
        let before = self.saved.len();
        self.saved.retain(|s| s.id != id);
        self.saved.len() != before
    }

    /// The entries, the one saved last at the end.
    pub fn entries(&self) -> &[Saved] {
        &self.saved
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

    /// The ids of the monitors connected: those driven, then those idle.
    pub fn monitor_ids(&self) -> Vec<String> {
        let idle = self.udev.as_ref().map(|u| u.idle_monitors()).unwrap_or_default();
        self.outputs
            .iter()
            .map(id_of)
            .chain(idle.into_iter().map(|m| m.id))
            .collect()
    }

    /// Whether each of `monitor_ids` goes by a saved entry (ADR-0124):
    /// its own or one of its model's.
    fn known_flags(&self) -> Vec<bool> {
        self.displays
            .resolve(&self.monitor_ids())
            .into_iter()
            .map(|e| e.is_some())
            .collect()
    }

    /// Whether the monitor `id`, connected, goes by a saved entry.
    pub fn is_known(&self, id: &str) -> bool {
        let ids = self.monitor_ids();
        ids.iter()
            .position(|i| i == id)
            .is_some_and(|at| self.displays.resolve(&ids)[at].is_some())
    }

    /// The monitors: those driven, the primary first, then those idle.
    pub fn monitors(&self) -> Vec<dbus::Monitor> {
        let ids = self.monitor_ids();
        let places = self.displays.places(&ids);
        let known = self.known_flags();
        let mut list: Vec<dbus::Monitor> = self
            .outputs
            .iter()
            .enumerate()
            .map(|(i, o)| {
                let area = self.space.output_geometry(o).unwrap_or_default();
                let place = places[i].as_ref();
                let props = o.physical_properties();
                let size = o.current_mode().map(|m| m.size).unwrap_or_default();
                dbus::Monitor {
                    name: o.name(),
                    known: known[i],
                    active: true,
                    to: place.map(|p| p.to.clone()).unwrap_or_default(),
                    side: place.map(|p| p.side.name().into()).unwrap_or_default(),
                    offset: place.map_or(0, |p| p.offset),
                    id: ids[i].clone(),
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
            .collect();
        let idle = self.udev.as_ref().map(|u| u.idle_monitors()).unwrap_or_default();
        for (j, m) in idle.into_iter().enumerate() {
            let i = self.outputs.len() + j;
            list.push(dbus::Monitor {
                known: known[i],
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
            });
        }
        list
    }

    /// The saved entries, for the Displays page: each with whether a
    /// connected monitor goes by it, and the name of the monitor it's next
    /// to, as far as that's known.
    pub fn saved_displays(&self) -> Vec<dbus::SavedDisplay> {
        let ids = self.monitor_ids();
        let by = self.displays.resolve(&ids);
        let monitors = self.monitors();
        let entries = self.displays.entries();
        entries
            .iter()
            .enumerate()
            .map(|(e, s)| {
                // What the neighbour is called: a connected monitor's, else
                // its own entry's, else what its id says.
                let to_name = monitors
                    .iter()
                    .find(|m| m.id == s.to)
                    .map(monitor_label)
                    .or_else(|| {
                        entries
                            .iter()
                            .find(|o| o.id == s.to)
                            .map(|o| o.name.clone())
                            .filter(|n| !n.is_empty())
                    })
                    .unwrap_or_else(|| model_key(&s.to).to_string());
                dbus::SavedDisplay {
                    id: s.id.clone(),
                    name: if s.name.is_empty() {
                        model_key(&s.id).to_string()
                    } else {
                        s.name.clone()
                    },
                    in_use: by.contains(&Some(e)),
                    to: s.to.clone(),
                    to_name: if s.to.is_empty() { String::new() } else { to_name },
                    side: s.side.clone(),
                    offset: s.offset,
                }
            })
            .collect()
    }

    /// A request from the bus.
    pub fn displays_request(&mut self, request: dbus::Request) {
        use dbus::Request;
        match request {
            Request::List(reply) => {
                let _ = reply.send(self.monitors());
            }
            Request::Saved(reply) => {
                let _ = reply.send(self.saved_displays());
            }
            Request::Forget(id, reply) => {
                let _ = reply.send(if self.displays.forget(&id) {
                    self.displays.save();
                    self.monitors_changed();
                    Ok(())
                } else {
                    Err(format!("no saved display {id}"))
                });
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
        let name = self
            .outputs
            .iter()
            .find(|o| id_of(o) == id)
            .map(label_of)
            .unwrap_or_default();
        self.displays.set(id, &name, place);
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
        // Known: it goes by a saved entry, its own or its model's.
        let known: Vec<String> = added
            .iter()
            .map(|(id, _)| id.clone())
            .filter(|id| self.is_known(id))
            .collect();
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
                service.added(id, name, known.contains(id));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LAPTOP: &str = "IVO-8C44-00000000-eDP-1";

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn place(to: &str, side: Side, offset: i32) -> Option<Place> {
        Some(Place {
            to: to.into(),
            side,
            offset,
        })
    }

    #[test]
    fn a_model_is_an_id_without_its_serial() {
        assert_eq!(model_key("GSM-772E-0005F79F"), "GSM-772E");
        assert_eq!(model_key("DEL-A0D3-7HK3B93"), "DEL-A0D3");
        assert_eq!(model_key(LAPTOP), "IVO-8C44");
        // Not maker, product and serial: its own model.
        assert_eq!(model_key("DP-3"), "DP-3");
        assert_eq!(model_key("eDP-1"), "eDP-1");
    }

    #[test]
    fn a_monitor_is_new_until_it_is_placed() {
        let mut d = Displays::default();
        let now = ids(&[LAPTOP, "GSM-772E-AAA"]);
        assert_eq!(d.resolve(&now), vec![None, None]);
        assert_eq!(d.places(&now), vec![None, None]);
        d.set("GSM-772E-AAA", "LG HDR WQHD", place(LAPTOP, Side::Above, 40));
        assert_eq!(d.resolve(&now), vec![None, Some(0)]);
        let places = d.places(&now);
        assert_eq!(places[0], None);
        let p = places[1].clone().unwrap();
        assert_eq!((p.to.as_str(), p.side, p.offset), (LAPTOP, Side::Above, 40));
    }

    #[test]
    fn another_of_the_model_goes_where_one_was_put() {
        // A hot desk: a Dell with a serial never seen goes where a Dell went.
        let mut d = Displays::default();
        d.set("DEL-A0D3-HOME1", "DELL U2720Q", place(LAPTOP, Side::RightOf, 0));
        let desk = ids(&[LAPTOP, "DEL-A0D3-DESK9"]);
        assert_eq!(d.resolve(&desk), vec![None, Some(0)]);
        let p = d.places(&desk)[1].clone().unwrap();
        assert_eq!((p.to.as_str(), p.side), (LAPTOP, Side::RightOf));
        // A different model doesn't.
        assert_eq!(d.resolve(&ids(&[LAPTOP, "GSM-772E-AAA"])), vec![None, None]);
    }

    #[test]
    fn a_monitor_goes_by_its_own_entry_before_its_models() {
        let mut d = Displays::default();
        d.set("DEL-A0D3-HOME1", "DELL", place(LAPTOP, Side::LeftOf, 0));
        d.set("DEL-A0D3-HOME2", "DELL", place(LAPTOP, Side::RightOf, 0));
        // HOME1 is the older entry and is connected: it keeps its own, and
        // the newer one doesn't take it just for being newer.
        let both = ids(&[LAPTOP, "DEL-A0D3-HOME1"]);
        assert_eq!(d.resolve(&both), vec![None, Some(0)]);
        assert_eq!(d.places(&both)[1].clone().unwrap().side, Side::LeftOf);
    }

    #[test]
    fn two_of_a_model_take_two_places_the_one_saved_last_first() {
        let mut d = Displays::default();
        d.set("DEL-A0D3-HOME1", "DELL", place(LAPTOP, Side::LeftOf, 0));
        d.set("DEL-A0D3-HOME2", "DELL", place(LAPTOP, Side::RightOf, 0));
        let desk = ids(&[LAPTOP, "DEL-A0D3-X", "DEL-A0D3-Y", "DEL-A0D3-Z"]);
        // X takes the last saved, Y the one before, and Z has none left.
        assert_eq!(d.resolve(&desk), vec![None, Some(1), Some(0), None]);
        let places = d.places(&desk);
        assert_eq!(places[1].clone().unwrap().side, Side::RightOf);
        assert_eq!(places[2].clone().unwrap().side, Side::LeftOf);
        assert_eq!(places[3], None);
    }

    #[test]
    fn a_neighbour_is_the_monitor_that_goes_by_its_entry() {
        // At home: Dell B is right of Dell A, which is right of the laptop.
        let mut d = Displays::default();
        d.set("DEL-A0D3-A", "DELL", place(LAPTOP, Side::RightOf, 0));
        d.set("DEL-A0D3-B", "DELL", place("DEL-A0D3-A", Side::RightOf, 0));
        // At a desk, two Dells never seen: B' goes by B's entry (saved last)
        // and A' by A's, and B' is next to A'.
        let desk = ids(&[LAPTOP, "DEL-A0D3-P", "DEL-A0D3-Q"]);
        let places = d.places(&desk);
        let p = places[1].clone().unwrap();
        assert_eq!((p.to.as_str(), p.side), ("DEL-A0D3-Q", Side::RightOf));
        let q = places[2].clone().unwrap();
        assert_eq!((q.to.as_str(), q.side), (LAPTOP, Side::RightOf));
    }

    #[test]
    fn a_neighbour_that_isnt_connected_leaves_it_with_no_place() {
        let mut d = Displays::default();
        d.set("DEL-A0D3-B", "DELL", place("GSM-772E-GONE", Side::RightOf, 0));
        let now = ids(&[LAPTOP, "DEL-A0D3-B"]);
        assert_eq!(d.resolve(&now)[1], Some(0));
        assert_eq!(d.places(&now)[1], None);
    }

    #[test]
    fn setting_again_moves_it_to_the_end_and_forgetting_removes_it() {
        let mut d = Displays::default();
        d.set("A-1", "a", None);
        d.set("B-1", "b", None);
        d.set("A-1", "a", place(LAPTOP, Side::Below, 0));
        let order: Vec<_> = d.entries().iter().map(|s| s.id.as_str()).collect();
        assert_eq!(order, ["B-1", "A-1"]);
        assert!(d.forget("B-1"));
        assert!(!d.forget("B-1"));
        assert_eq!(d.entries().len(), 1);
    }

    #[test]
    fn what_is_kept_reads_back() {
        let mut d = Displays {
            mode: Mode::Mirror,
            ..Displays::default()
        };
        d.set("GSM-772E-AAA", "LG HDR WQHD", place(LAPTOP, Side::LeftOf, -8));
        let back: File = toml::from_str(&d.file()).unwrap();
        assert_eq!(Mode::from_name(&back.mode), Some(Mode::Mirror));
        assert_eq!(back.monitor, d.saved);
        assert_eq!(back.monitor[0].name, "LG HDR WQHD");
        // A file from before names were kept still reads.
        let old: File =
            toml::from_str("[[monitor]]\nid = \"GSM-772E-AAA\"\nside = \"above\"\n").unwrap();
        assert_eq!(old.monitor[0].name, "");
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
