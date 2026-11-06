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

/// How much of a hidden element stays on screen to be touched, in logical
/// pixels.
pub const SLIVER: i32 = 2;
/// How long an element takes to slide in or out.
const SLIDE: Duration = Duration::from_millis(140);
/// How long one stays out after the pointer leaves it.
const LINGER: Duration = Duration::from_millis(600);

fn state_path() -> Option<PathBuf> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from)?;
    Some(dir.join("patina-hud").join("hidden"))
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

/// What an element is doing now, with the HUD hidden or not.
pub fn effective(mode: Visibility, all_hidden: bool) -> Visibility {
    match mode {
        Visibility::Always if all_hidden => Visibility::Autohide,
        mode => mode,
    }
}

/// How far out an element is, from 0 (off screen but for the sliver) to 1
/// (in place), moving toward where it should be.
#[derive(Debug, Clone, Copy)]
pub struct Slide {
    out: f32,
    last: Option<Instant>,
    /// Until when the pointer counts as being on it.
    until: Option<Instant>,
}

impl Default for Slide {
    fn default() -> Self {
        Self {
            out: 1.0,
            last: None,
            until: None,
        }
    }
}

impl Slide {
    pub fn out(&self) -> f32 {
        self.out
    }

    /// Moves on to `now`; `mode` is the element's, `hover` whether the
    /// pointer is on it. Returns whether it's still moving, or waiting to.
    pub fn step(&mut self, mode: Visibility, hover: bool, now: Instant) -> bool {
        if hover {
            self.until = Some(now + LINGER);
        }
        let held = self.until.is_some_and(|t| t > now);
        let target = match mode {
            Visibility::Always => 1.0,
            Visibility::Autohide if held => 1.0,
            Visibility::Autohide | Visibility::Hidden => 0.0,
        };
        let dt = self
            .last
            .map_or(Duration::ZERO, |t| now.saturating_duration_since(t));
        self.last = Some(now);
        let by = dt.as_secs_f32() / SLIDE.as_secs_f32();
        if self.out < target {
            self.out = (self.out + by.max(0.2)).min(target);
        } else if self.out > target {
            self.out = (self.out - by.max(0.2)).max(target);
        }
        // A part waiting out its linger needs another frame to leave.
        self.out != target || (mode == Visibility::Autohide && held)
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
        for _ in 0..20 {
            now += Duration::from_millis(16);
            s.step(Visibility::Autohide, false, now);
        }
        assert_eq!(s.out(), 0.0);
        // The pointer on the sliver brings it in, and it stays a while.
        for _ in 0..20 {
            now += Duration::from_millis(16);
            s.step(Visibility::Autohide, true, now);
        }
        assert_eq!(s.out(), 1.0);
        now += Duration::from_millis(100);
        s.step(Visibility::Autohide, false, now);
        assert_eq!(s.out(), 1.0, "it lingers");
        for _ in 0..80 {
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
