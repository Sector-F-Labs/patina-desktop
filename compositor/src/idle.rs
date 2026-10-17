//! The screen turning off when nothing's been touched (ADR-0047): after
//! `[screen] off_after` seconds without input, the monitor is powered off
//! (DPMS, on the hardware), and any input turns it back on. Nested, the
//! parent desktop looks after its own screen, so it's only logged.
//!
//! What's playing holds it on: a surface with the idle-inhibit protocol
//! (video players, browsers), or a holder of `org.freedesktop.ScreenSaver`'s
//! Inhibit on the session bus (`dbus.rs`; Steam, while a game runs, whose
//! gamepad the compositor never sees).

pub mod dbus;

use std::time::{Duration, Instant};

use smithay::{
    delegate_idle_inhibit,
    reexports::{
        calloop::timer::{TimeoutAction, Timer},
        wayland_server::protocol::wl_surface::WlSurface,
    },
    utils::IsAlive,
    wayland::idle_inhibit::IdleInhibitHandler,
};

use crate::state::State;

/// How often the time without input is looked at.
const TICK: Duration = Duration::from_secs(1);

/// Whether the screen is off, and what holds it on.
pub struct Idle {
    /// The last input, or the last moment something held the screen on.
    last: Instant,
    off: bool,
    /// Surfaces inhibiting idle through the protocol.
    surfaces: Vec<WlSurface>,
    /// Inhibitors from the session bus.
    held: Vec<dbus::Held>,
    service: Option<dbus::Service>,
}

impl Default for Idle {
    fn default() -> Self {
        Self {
            last: Instant::now(),
            off: false,
            surfaces: Vec::new(),
            held: Vec::new(),
            service: None,
        }
    }
}

impl Idle {
    /// Something holds the screen on.
    fn inhibited(&mut self) -> bool {
        self.surfaces.retain(|s| s.alive());
        !self.surfaces.is_empty() || !self.held.is_empty()
    }

    /// Whether it's time to turn the screen off: `off_after` seconds (0
    /// for never) since the last input, at `now`, with nothing holding it
    /// on. A holder counts as input, so its going starts the wait afresh.
    fn due(&mut self, off_after: u64, now: Instant) -> bool {
        if self.off || off_after == 0 {
            return false;
        }
        if self.inhibited() {
            self.last = now;
            return false;
        }
        now.duration_since(self.last) >= Duration::from_secs(off_after)
    }
}

impl State {
    /// Starts counting, and serves the session bus's ScreenSaver.
    pub fn start_idle(&mut self) {
        let _ = self
            .loop_handle
            .insert_source(Timer::from_duration(TICK), |_, _, state| {
                state.idle_tick();
                TimeoutAction::ToDuration(TICK)
            });
        match dbus::Service::start(&self.loop_handle) {
            Ok(service) => self.idle.service = Some(service),
            Err(e) => eprintln!("patina-compositor: org.freedesktop.ScreenSaver isn't served: {e}"),
        }
    }

    /// Input: the screen comes back on if it was off.
    pub fn input_activity(&mut self) {
        self.idle.last = Instant::now();
        if self.idle.off {
            self.screen_on();
        }
    }

    fn idle_tick(&mut self) {
        if self.idle.due(self.config.screen.off_after, Instant::now()) {
            self.screen_off();
        }
    }

    fn screen_off(&mut self) {
        eprintln!(
            "patina-compositor: no input for {}s; turning the screen off",
            self.config.screen.off_after
        );
        if self.config.lock.on_screen_off {
            self.lock_screen();
        }
        self.idle.off = true;
        if let Some(u) = self.udev.as_mut() {
            u.power_off();
        }
        self.publish_idle();
    }

    fn screen_on(&mut self) {
        eprintln!("patina-compositor: input; turning the screen on");
        self.idle.off = false;
        if let Some(u) = self.udev.as_mut() {
            u.power_on();
        }
        self.needs_redraw = true;
        self.publish_idle();
    }

    fn publish_idle(&self) {
        if let Some(service) = &self.idle.service {
            service.publish(self.idle.off);
        }
    }

    /// A request from the session bus.
    fn idle_request(&mut self, request: dbus::Request) {
        match request {
            dbus::Request::Inhibit(held) => {
                eprintln!(
                    "patina-compositor: {:?} holds the screen on: {:?}",
                    held.app, held.reason
                );
                self.idle.held.push(held);
            }
            dbus::Request::Uninhibit { cookie, owner } => {
                self.idle
                    .held
                    .retain(|h| !(h.cookie == cookie && h.owner == owner));
                self.idle.last = Instant::now();
            }
            dbus::Request::Gone(owner) => {
                let before = self.idle.held.len();
                self.idle.held.retain(|h| h.owner != owner);
                if self.idle.held.len() != before {
                    self.idle.last = Instant::now();
                }
            }
            dbus::Request::Activity => self.input_activity(),
        }
    }
}

impl IdleInhibitHandler for State {
    fn inhibit(&mut self, surface: WlSurface) {
        if !self.idle.surfaces.contains(&surface) {
            self.idle.surfaces.push(surface);
        }
    }

    fn uninhibit(&mut self, surface: WlSurface) {
        self.idle.surfaces.retain(|s| *s != surface);
        self.idle.last = Instant::now();
    }
}

delegate_idle_inhibit!(State);

#[cfg(test)]
mod tests {
    use super::*;

    fn held() -> dbus::Held {
        dbus::Held {
            cookie: 1,
            owner: ":1.5".into(),
            app: "Steam".into(),
            reason: "game".into(),
        }
    }

    #[test]
    fn it_turns_off_after_the_wait_without_input() {
        let mut idle = Idle::default();
        let start = idle.last;
        assert!(!idle.due(300, start + Duration::from_secs(299)));
        assert!(idle.due(300, start + Duration::from_secs(300)));
        // Never, with 0.
        assert!(!idle.due(0, start + Duration::from_secs(3600)));
        // Not again while it's off.
        idle.off = true;
        assert!(!idle.due(300, start + Duration::from_secs(900)));
    }

    #[test]
    fn a_holder_keeps_it_on_and_its_going_starts_the_wait_afresh() {
        let mut idle = Idle::default();
        let start = idle.last;
        idle.held.push(held());
        assert!(!idle.due(60, start + Duration::from_secs(600)));
        idle.held.clear();
        // The wait counts from when it was last held, not the last input.
        assert!(!idle.due(60, start + Duration::from_secs(630)));
        assert!(idle.due(60, start + Duration::from_secs(661)));
    }
}
