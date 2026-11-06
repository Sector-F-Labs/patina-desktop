//! Hiding the HUD (ADR-0111): the state Super+H toggles, and the slide of
//! an element that hides off the screen's edge.
//!
//! The state is a file, `$XDG_RUNTIME_DIR/patina-hud/hidden`: `patina-hud
//! --toggle` makes it or removes it, and every HUD process looks for it a
//! few times a second. A file needs no connection between the processes,
//! and goes with the session.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::config::Visibility;

/// How much of a hidden element's surface stays on screen to be touched, in
/// logical pixels: a transparent strip along its edge, past the element
/// itself, which is where the pointer brings it out.
pub const HOT: i32 = 10;
/// How long an element takes to slide in or out.
const SLIDE: Duration = Duration::from_millis(260);
/// How long one stays out after the pointer leaves it.
const LINGER: Duration = Duration::from_millis(600);
/// How long one stays out when it came for a moment (a window change, the
/// launcher's shortcut).
const PEEK: Duration = Duration::from_millis(1400);

fn state_dir() -> Option<PathBuf> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from)?;
    Some(dir.join("patina-hud"))
}

fn state_path() -> Option<PathBuf> {
    Some(state_dir()?.join("hidden"))
}

/// Whether the HUD has been hidden with Super+H.
pub fn hidden() -> bool {
    state_path().is_some_and(|p| p.exists())
}

/// Hides the HUD, or brings it back; returns whether it's now hidden.
pub fn toggle() -> Result<bool, String> {
    let path = state_path().ok_or("no XDG_RUNTIME_DIR")?;
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        return Ok(false);
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(&path, "").map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(true)
}

/// Asks the element called `name` to come out for a moment: `patina-hud
/// --peek launcher`, which the compositor runs for the launcher's shortcut.
/// It's a file the element looks at, as for [`hidden`].
pub fn peek(name: &str) -> Result<(), String> {
    let dir = state_dir().ok_or("no XDG_RUNTIME_DIR")?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = dir.join(format!("peek-{name}"));
    // The file's time is the ask, so each one is a new time.
    let _ = std::fs::remove_file(&path);
    std::fs::write(&path, "").map_err(|e| format!("{}: {e}", path.display()))
}

/// When `name` was last asked to peek.
pub fn peeked(name: &str) -> Option<std::time::SystemTime> {
    let path = state_dir()?.join(format!("peek-{name}"));
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// What an element is doing now, with the HUD hidden or not.
pub fn effective(mode: Visibility, all_hidden: bool) -> Visibility {
    match mode {
        Visibility::Always if all_hidden => Visibility::Autohide,
        mode => mode,
    }
}

fn ease(p: f32) -> f32 {
    // Quick out of the gate, settling at the end.
    1.0 - (1.0 - p).powi(3)
}

/// How far out an element is, from 0 (off screen but for the strip) to 1
/// (in place), eased toward where it should be.
#[derive(Debug, Clone, Copy)]
pub struct Slide {
    out: f32,
    from: f32,
    to: f32,
    started: Option<Instant>,
    /// Until when the pointer, or a peek, keeps it out.
    until: Option<Instant>,
}

impl Default for Slide {
    fn default() -> Self {
        Self {
            out: 1.0,
            from: 1.0,
            to: 1.0,
            started: None,
            until: None,
        }
    }
}

impl Slide {
    pub fn out(&self) -> f32 {
        self.out
    }

    /// Comes out for a moment.
    pub fn peek(&mut self, now: Instant) {
        self.until = Some(self.until.map_or(now + PEEK, |t| t.max(now + PEEK)));
    }

    /// Moves on to `now`; `mode` is the element's, `hover` whether the
    /// pointer is on it. Returns whether it's still moving, or waiting to.
    pub fn step(&mut self, mode: Visibility, hover: bool, now: Instant) -> bool {
        if hover {
            self.until = Some(self.until.map_or(now + LINGER, |t| t.max(now + LINGER)));
        }
        let held = self.until.is_some_and(|t| t > now);
        let target = match mode {
            Visibility::Always => 1.0,
            Visibility::Autohide if held => 1.0,
            Visibility::Autohide | Visibility::Hidden => 0.0,
        };
        if target != self.to {
            self.from = self.out;
            self.to = target;
            self.started = Some(now);
        }
        let progress = self.started.map_or(1.0, |t| {
            (now.saturating_duration_since(t).as_secs_f32() / SLIDE.as_secs_f32()).min(1.0)
        });
        self.out = self.from + (self.to - self.from) * ease(progress);
        if progress >= 1.0 {
            self.out = self.to;
        }
        // Waiting out a linger needs another frame to leave.
        progress < 1.0 || (mode == Visibility::Autohide && held)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn always_stays_in_place() {
        let mut s = Slide::default();
        assert!(!s.step(Visibility::Always, false, Instant::now()));
        assert_eq!(s.out(), 1.0);
    }

    #[test]
    fn autohide_slides_out_and_in_with_the_pointer() {
        let mut s = Slide::default();
        let t = Instant::now();
        let mut now = t;
        // No pointer: goes out.
        for _ in 0..40 {
            now += Duration::from_millis(16);
            s.step(Visibility::Autohide, false, now);
        }
        assert_eq!(s.out(), 0.0);
        // The pointer on the strip brings it in, and it stays a while.
        for _ in 0..40 {
            now += Duration::from_millis(16);
            s.step(Visibility::Autohide, true, now);
        }
        assert_eq!(s.out(), 1.0);
        now += Duration::from_millis(100);
        s.step(Visibility::Autohide, false, now);
        assert_eq!(s.out(), 1.0, "it lingers");
        for _ in 0..120 {
            now += Duration::from_millis(16);
            s.step(Visibility::Autohide, false, now);
        }
        assert_eq!(s.out(), 0.0);
    }

    #[test]
    fn it_eases_rather_than_jumping() {
        let mut s = Slide::default();
        let mut now = Instant::now();
        let mut seen = Vec::new();
        for _ in 0..30 {
            now += Duration::from_millis(16);
            s.step(Visibility::Autohide, false, now);
            seen.push(s.out());
        }
        // Many steps on the way, each smaller than the one before.
        let moving = seen.iter().filter(|o| **o > 0.0 && **o < 1.0).count();
        assert!(moving >= 8, "{seen:?}");
        let steps: Vec<f32> = seen.windows(2).map(|w| w[0] - w[1]).collect();
        assert!(steps[1] > steps[steps.len() / 2], "{steps:?}");
    }

    #[test]
    fn a_peek_brings_it_out_and_it_goes_again() {
        let mut s = Slide::default();
        let mut now = Instant::now();
        for _ in 0..40 {
            now += Duration::from_millis(16);
            s.step(Visibility::Autohide, false, now);
        }
        assert_eq!(s.out(), 0.0);
        s.peek(now);
        for _ in 0..40 {
            now += Duration::from_millis(16);
            s.step(Visibility::Autohide, false, now);
        }
        assert_eq!(s.out(), 1.0);
        for _ in 0..200 {
            now += Duration::from_millis(16);
            s.step(Visibility::Autohide, false, now);
        }
        assert_eq!(s.out(), 0.0);
    }

    #[test]
    fn hidden_never_comes_out() {
        let mut s = Slide::default();
        let mut now = Instant::now();
        for _ in 0..20 {
            now += Duration::from_millis(16);
            s.step(Visibility::Hidden, true, now);
        }
        assert_eq!(s.out(), 0.0);
    }

    #[test]
    fn all_hidden_hides_only_what_is_shown() {
        assert_eq!(effective(Visibility::Always, true), Visibility::Autohide);
        assert_eq!(effective(Visibility::Hidden, true), Visibility::Hidden);
        assert_eq!(effective(Visibility::Always, false), Visibility::Always);
    }
}
