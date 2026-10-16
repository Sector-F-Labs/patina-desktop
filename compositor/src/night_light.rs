//! Night Light (ADR-0039): the screen warmed in the evening through the
//! CRTC's gamma ramp, on the hardware only.
//!
//! The tint is on when `[night_light]` and its schedule say so (or an
//! override from outside does), and nothing inhibits it. Changes fade: over a
//! minute at a schedule's edge, in a second otherwise. The ramp is set by the
//! udev backend (`UdevData::set_temperature`); nested, the parent desktop has
//! its own night light and this does nothing. Outside control (inhibitors,
//! overrides) comes over D-Bus, in `night_light/dbus.rs`.

mod dbus;

use std::collections::BTreeMap;
use std::time::{Duration, Instant, SystemTime};

use chrono::{NaiveDateTime, NaiveTime};
use smithay::reexports::calloop::timer::{TimeoutAction, Timer};

use crate::config::{self, NightLightMode};
use crate::state::State;

pub use dbus::Request;

/// The temperature that leaves the ramp as it is.
pub const NEUTRAL: u32 = 6500;
/// The warmest the setting goes.
pub const WARMEST: u32 = 1700;

/// How long a fade takes at a schedule's edge, as the evening comes.
const EDGE_FADE: Duration = Duration::from_secs(60);
/// And when it's asked for: a setting, an override or an app inhibiting.
const QUICK_FADE: Duration = Duration::from_secs(1);
/// How often the schedule and the config file are looked at, as the bar
/// looks at its own.
const POLL: Duration = Duration::from_secs(1);
/// How often a fade steps.
const STEP: Duration = Duration::from_millis(50);

/// What each channel is multiplied by at `kelvin`, the brightest at 1: a
/// blackbody's colour by Tanner Helland's fit (as redshift and gammastep
/// use), relative to its colour at 6500 K, so 6500 K changes nothing.
pub fn white_point(kelvin: f64) -> [f64; 3] {
    let raw = |k: f64| {
        let t = k / 100.0;
        let red = if t <= 66.0 {
            255.0
        } else {
            329.698_727_446 * (t - 60.0).powf(-0.133_204_759_2)
        };
        let green = if t <= 66.0 {
            99.470_802_586_1 * t.ln() - 161.119_568_166_1
        } else {
            288.122_169_528_3 * (t - 60.0).powf(-0.075_514_849_2)
        };
        let blue = if t >= 66.0 {
            255.0
        } else if t <= 19.0 {
            0.0
        } else {
            138.517_731_223_1 * (t - 10.0).ln() - 305.044_792_730_7
        };
        [red, green, blue].map(|c| c.clamp(0.0, 255.0))
    };
    let kelvin = kelvin.clamp(1000.0, NEUTRAL as f64);
    let (warm, neutral) = (raw(kelvin), raw(NEUTRAL as f64));
    let relative: [f64; 3] = std::array::from_fn(|i| warm[i] / neutral[i]);
    let brightest = relative.iter().cloned().fold(f64::MIN, f64::max);
    relative.map(|c| (c / brightest).clamp(0.0, 1.0))
}

/// `original`'s red, green and blue ramps warmed to `kelvin`: each scaled
/// by its channel, so a calibrated ramp stays calibrated.
pub fn warm_ramps(original: &[Vec<u16>; 3], kelvin: f64) -> [Vec<u16>; 3] {
    let point = white_point(kelvin);
    std::array::from_fn(|c| {
        original[c]
            .iter()
            .map(|&v| (v as f64 * point[c]).round() as u16)
            .collect()
    })
}

/// When the screen is warm. Manual times for now; a sunset-to-sunrise
/// schedule is another of these.
pub trait Schedule {
    /// Whether it's warm at `now`, and when that next changes (`None` for
    /// never).
    fn at(&self, now: NaiveDateTime) -> (bool, Option<NaiveDateTime>);
}

/// Always the same.
struct Constant(bool);

impl Schedule for Constant {
    fn at(&self, _: NaiveDateTime) -> (bool, Option<NaiveDateTime>) {
        (self.0, None)
    }
}

/// Warm from `from` to `to` every day, past midnight when `to` is earlier.
/// The same time for both is never warm.
pub struct Manual {
    pub from: NaiveTime,
    pub to: NaiveTime,
}

impl Schedule for Manual {
    fn at(&self, now: NaiveDateTime) -> (bool, Option<NaiveDateTime>) {
        if self.from == self.to {
            return (false, None);
        }
        let t = now.time();
        let warm = if self.from < self.to {
            self.from <= t && t < self.to
        } else {
            t >= self.from || t < self.to
        };
        let edge = if warm { self.to } else { self.from };
        (warm, Some(next(now, edge)))
    }
}

/// The next time after `now` the clock shows `time`.
fn next(now: NaiveDateTime, time: NaiveTime) -> NaiveDateTime {
    let today = now.date().and_time(time);
    if today > now {
        today
    } else {
        today + chrono::Duration::days(1)
    }
}

/// The schedule `settings` describe.
fn schedule(settings: &config::NightLight) -> Box<dyn Schedule> {
    match settings.mode {
        NightLightMode::Off => Box::new(Constant(false)),
        NightLightMode::Always => Box::new(Constant(true)),
        NightLightMode::Schedule => {
            match (
                config::time_of_day(&settings.from),
                config::time_of_day(&settings.to),
            ) {
                (Some(from), Some(to)) => Box::new(Manual { from, to }),
                // The config's checks rule this out.
                _ => Box::new(Constant(false)),
            }
        }
    }
}

/// An override from outside, which lasts until the schedule next changes
/// (or, with no schedule, until it's taken back or the settings change).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Override {
    Auto,
    On,
    Off,
}

impl Override {
    pub fn name(self) -> &'static str {
        match self {
            Override::Auto => "auto",
            Override::On => "on",
            Override::Off => "off",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "auto" => Some(Override::Auto),
            "on" => Some(Override::On),
            "off" => Some(Override::Off),
            _ => None,
        }
    }
}

/// Someone outside holding Night Light off, until they let go or leave the
/// bus.
#[derive(Debug, Clone)]
struct Inhibitor {
    /// Their unique bus name.
    owner: String,
    reason: String,
}

#[derive(Debug, Clone, Copy)]
struct Fade {
    from: f64,
    to: f64,
    start: Instant,
    length: Duration,
}

impl Fade {
    fn at(&self, now: Instant) -> f64 {
        let t = (now - self.start).as_secs_f64() / self.length.as_secs_f64().max(1e-9);
        self.from + (self.to - self.from) * t.clamp(0.0, 1.0)
    }

    fn done(&self, now: Instant) -> bool {
        now - self.start >= self.length
    }
}

/// Night Light's state; the compositor's half of it lives in `State`.
pub struct NightLight {
    settings: config::NightLight,
    schedule: Box<dyn Schedule>,
    overridden: Override,
    /// When the override runs out: the schedule's next change when it was
    /// set.
    override_until: Option<NaiveDateTime>,
    inhibitors: BTreeMap<u32, Inhibitor>,
    /// The temperature on screen.
    shown: f64,
    /// Where it's heading.
    target: f64,
    fade: Option<Fade>,
    /// The schedule's last answer, to tell an edge passing from a change
    /// asked for.
    scheduled: Option<bool>,
    /// A step timer is running.
    stepping: bool,
    /// The config file's last modification time, to reload it on a change.
    config_mtime: Option<SystemTime>,
    /// The D-Bus side, on the hardware.
    service: Option<dbus::Service>,
}

impl NightLight {
    pub fn new(settings: &config::NightLight) -> Self {
        Self {
            settings: settings.clone(),
            schedule: schedule(settings),
            overridden: Override::Auto,
            override_until: None,
            inhibitors: BTreeMap::new(),
            shown: NEUTRAL as f64,
            target: NEUTRAL as f64,
            fade: None,
            scheduled: None,
            stepping: false,
            config_mtime: None,
            service: None,
        }
    }

    /// New settings, from a reload. A changed setting ends an override.
    pub fn configure(&mut self, settings: &config::NightLight) {
        if *settings != self.settings {
            self.settings = settings.clone();
            self.schedule = schedule(settings);
            self.overridden = Override::Auto;
            self.override_until = None;
            self.scheduled = None;
        }
    }

    /// Whether the schedule, and any override, want it warm at `now`.
    fn wanted(&mut self, now: NaiveDateTime) -> bool {
        if self.override_until.is_some_and(|until| now >= until) {
            self.overridden = Override::Auto;
            self.override_until = None;
        }
        match self.overridden {
            Override::On => true,
            Override::Off => false,
            Override::Auto => self.schedule.at(now).0,
        }
    }

    /// Whether it's warm now: wanted, and not inhibited.
    pub fn active(&mut self, now: NaiveDateTime) -> bool {
        self.wanted(now) && self.inhibitors.is_empty()
    }

    /// The temperature to head for at `now`, and how fast: slowly when the
    /// schedule's edge has just passed, quickly otherwise.
    fn aim(&mut self, now: NaiveDateTime) -> (f64, Duration) {
        let scheduled = self.schedule.at(now).0;
        let edge = self.scheduled.is_some_and(|was| was != scheduled);
        self.scheduled = Some(scheduled);
        let target = if self.active(now) {
            self.settings.temperature as f64
        } else {
            NEUTRAL as f64
        };
        let length = if edge && self.overridden == Override::Auto {
            EDGE_FADE
        } else {
            QUICK_FADE
        };
        (target, length)
    }

    /// Heads for the right temperature at `now`; true if a fade is under
    /// way.
    fn update(&mut self, now: NaiveDateTime, instant: Instant) -> bool {
        let (target, length) = self.aim(now);
        if target != self.target {
            self.target = target;
            self.fade = Some(Fade {
                from: self.shown,
                to: target,
                start: instant,
                length,
            });
        }
        self.fade.is_some()
    }

    /// Moves the fade on; the temperature to show, and whether there's more
    /// to come.
    fn step(&mut self, instant: Instant) -> (f64, bool) {
        match self.fade {
            Some(fade) => {
                self.shown = fade.at(instant);
                let done = fade.done(instant);
                if done {
                    self.shown = fade.to;
                    self.fade = None;
                }
                (self.shown, !done)
            }
            None => (self.shown, false),
        }
    }

    fn set_override(&mut self, overridden: Override, now: NaiveDateTime) {
        self.overridden = overridden;
        self.override_until = match overridden {
            Override::Auto => None,
            _ => self.schedule.at(now).1,
        };
    }

    fn status(&mut self, available: bool, now: NaiveDateTime) -> dbus::Status {
        dbus::Status {
            available,
            active: self.active(now),
            inhibitors: self.inhibitors.values().map(|i| i.reason.clone()).collect(),
            temperature: self.shown.round() as u32,
            overridden: self.overridden.name().into(),
        }
    }
}

fn local_now() -> NaiveDateTime {
    chrono::Local::now().naive_local()
}

fn config_mtime() -> Option<SystemTime> {
    config::path()
        .and_then(|p| std::fs::metadata(p).ok())
        .and_then(|m| m.modified().ok())
}

impl State {
    /// Starts watching the config file, the schedule and, on the hardware,
    /// the D-Bus service; called once at start-up.
    pub fn start_night_light(&mut self) {
        self.night_light.config_mtime = config_mtime();
        let _ = self
            .loop_handle
            .insert_source(Timer::from_duration(POLL), |_, _, state| {
                state.poll_config();
                state.night_light_update();
                TimeoutAction::ToDuration(POLL)
            });
        if self.udev.is_some() {
            match dbus::Service::start(&self.loop_handle) {
                Ok(service) => self.night_light.service = Some(service),
                Err(e) => eprintln!("patina-compositor: night light isn't on D-Bus: {e}"),
            }
        }
        self.night_light_update();
    }

    /// Reloads the config when the file changes, so Settings only has to
    /// write it.
    fn poll_config(&mut self) {
        let mtime = config_mtime();
        if mtime != self.night_light.config_mtime {
            self.night_light.config_mtime = mtime;
            self.reload_config(false);
        }
    }

    /// Heads for the temperature the settings, schedule, overrides and
    /// inhibitors call for, fading if it changes.
    pub fn night_light_update(&mut self) {
        let fading = self.night_light.update(local_now(), Instant::now());
        if fading && !self.night_light.stepping {
            self.night_light.stepping = true;
            let _ = self
                .loop_handle
                .insert_source(Timer::immediate(), |_, _, state| {
                    if state.night_light_step() {
                        TimeoutAction::ToDuration(STEP)
                    } else {
                        state.night_light.stepping = false;
                        TimeoutAction::Drop
                    }
                });
        }
        self.publish_night_light(false);
    }

    /// One step of a fade, on screen; true while there's more.
    fn night_light_step(&mut self) -> bool {
        let (kelvin, more) = self.night_light.step(Instant::now());
        if let Some(u) = self.udev.as_mut() {
            u.set_temperature(kelvin);
        }
        if !more {
            self.publish_night_light(true);
        }
        more
    }

    /// Tells the D-Bus side what's going on; `settled` at the end of a fade,
    /// when the temperature is worth announcing.
    fn publish_night_light(&mut self, settled: bool) {
        let available = self.udev.as_ref().is_some_and(|u| u.has_gamma());
        let status = self.night_light.status(available, local_now());
        if let Some(service) = &self.night_light.service {
            service.publish(status, settled);
        }
    }

    /// A request from D-Bus.
    pub fn night_light_request(&mut self, request: Request) {
        let night = &mut self.night_light;
        match request {
            Request::Inhibit {
                cookie,
                owner,
                reason,
            } => {
                eprintln!("patina-compositor: night light inhibited by {owner}: {reason}");
                night.inhibitors.insert(cookie, Inhibitor { owner, reason });
            }
            Request::Uninhibit { cookie, owner } => {
                if night
                    .inhibitors
                    .get(&cookie)
                    .is_some_and(|i| i.owner == owner)
                {
                    night.inhibitors.remove(&cookie);
                }
            }
            Request::Gone(owner) => night.inhibitors.retain(|_, i| i.owner != owner),
            Request::Override(o) => night.set_override(o, local_now()),
        }
        self.night_light_update();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(h: u32, m: u32) -> NaiveDateTime {
        chrono::NaiveDate::from_ymd_opt(2026, 10, 16)
            .unwrap()
            .and_hms_opt(h, m, 0)
            .unwrap()
    }

    fn time(h: u32, m: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(h, m, 0).unwrap()
    }

    #[test]
    fn neutral_changes_nothing_and_warmer_drops_blue_first() {
        assert_eq!(white_point(6500.0), [1.0, 1.0, 1.0]);
        let [r, g, b] = white_point(4000.0);
        assert_eq!(r, 1.0);
        assert!(g < 1.0 && b < g, "{g} {b}");
        let [_, g2, b2] = white_point(WARMEST as f64);
        assert!(g2 < g && b2 < b);
        // Warmer than the table goes is the warmest it has.
        assert_eq!(white_point(9000.0), [1.0, 1.0, 1.0]);
    }

    #[test]
    fn ramps_scale_the_original() {
        let identity: Vec<u16> = (0..256).map(|i| (i * 257) as u16).collect();
        let original = [identity.clone(), identity.clone(), identity];
        assert_eq!(warm_ramps(&original, 6500.0), original);
        let warm = warm_ramps(&original, 3000.0);
        assert_eq!(warm[0], original[0]);
        assert!(warm[2][255] < original[2][255]);
        assert_eq!(warm[2][0], 0);
    }

    #[test]
    fn a_schedule_past_midnight() {
        let s = Manual {
            from: time(20, 0),
            to: time(6, 0),
        };
        assert_eq!(s.at(at(12, 0)), (false, Some(at(20, 0))));
        assert!(s.at(at(20, 0)).0);
        let (warm, until) = s.at(at(23, 0));
        assert!(warm);
        assert_eq!(until, Some(at(6, 0) + chrono::Duration::days(1)));
        assert_eq!(s.at(at(5, 59)), (true, Some(at(6, 0))));
        assert!(!s.at(at(6, 0)).0);
    }

    #[test]
    fn a_schedule_within_a_day_and_an_empty_one() {
        let s = Manual {
            from: time(13, 0),
            to: time(14, 0),
        };
        assert!(s.at(at(13, 30)).0);
        assert!(!s.at(at(14, 30)).0);
        assert_eq!(
            s.at(at(14, 30)).1,
            Some(at(13, 0) + chrono::Duration::days(1))
        );
        let empty = Manual {
            from: time(9, 0),
            to: time(9, 0),
        };
        assert_eq!(empty.at(at(9, 0)), (false, None));
    }

    fn evening() -> config::NightLight {
        config::NightLight {
            mode: NightLightMode::Schedule,
            temperature: 3000,
            ..Default::default()
        }
    }

    #[test]
    fn an_inhibitor_holds_it_off_and_letting_go_brings_it_back() {
        let mut n = NightLight::new(&evening());
        let now = Instant::now();
        assert!(n.update(at(22, 0), now));
        assert_eq!(n.target, 3000.0);
        n.inhibitors.insert(
            1,
            Inhibitor {
                owner: ":1.5".into(),
                reason: "editing photos".into(),
            },
        );
        n.update(at(22, 0), now);
        assert_eq!(n.target, NEUTRAL as f64);
        assert!(!n.active(at(22, 0)));
        n.inhibitors.clear();
        n.update(at(22, 0), now);
        assert_eq!(n.target, 3000.0);
    }

    #[test]
    fn an_override_lasts_until_the_schedule_changes() {
        let mut n = NightLight::new(&evening());
        n.set_override(Override::Off, at(22, 0));
        assert!(!n.active(at(23, 0)));
        // Morning: the schedule turned it off, and the override is spent.
        assert!(!n.active(at(7, 0) + chrono::Duration::days(1)));
        assert_eq!(n.overridden, Override::Auto);
        assert!(n.active(at(21, 0) + chrono::Duration::days(1)));
        // On, in the afternoon: until the evening, when the schedule agrees.
        n.set_override(Override::On, at(15, 0));
        assert!(n.active(at(16, 0)));
        assert_eq!(n.override_until, Some(at(20, 0)));
    }

    #[test]
    fn new_settings_end_an_override() {
        let mut n = NightLight::new(&evening());
        n.set_override(Override::Off, at(22, 0));
        n.configure(&config::NightLight {
            temperature: 2500,
            ..evening()
        });
        assert_eq!(n.overridden, Override::Auto);
        assert!(n.active(at(22, 0)));
    }

    #[test]
    fn the_evening_fades_slowly_and_a_request_quickly() {
        let mut n = NightLight::new(&evening());
        let start = Instant::now();
        n.update(at(19, 59), start);
        assert!(n.fade.is_none());
        n.update(at(20, 0), start);
        assert_eq!(n.fade.unwrap().length, EDGE_FADE);
        let (k, more) = n.step(start + EDGE_FADE / 2);
        assert!(more);
        assert!((k - 4750.0).abs() < 1.0, "{k}");
        let (k, more) = n.step(start + EDGE_FADE);
        assert!(!more);
        assert_eq!(k, 3000.0);
        n.set_override(Override::Off, at(20, 1));
        n.update(at(20, 1), start);
        assert_eq!(n.fade.unwrap().length, QUICK_FADE);
    }
}
