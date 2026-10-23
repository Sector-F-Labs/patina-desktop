//! Moving between windows in the bar's tab order (ADR-0027): the bar's
//! tabs are the one list of open windows, so Alt+Tab steps along them, and
//! closing a window gives the keyboard back to the window it came from, or
//! else its neighbouring tab, as in a browser (ADR-0059), rather than a
//! separate switcher or a hidden most-recently-used order. Pinned apps come
//! first (ADR-0080), and the bar can move a window along (`dbus`).

pub mod dbus;

use std::cell::RefCell;

use smithay::desktop::Window;
use smithay::reexports::wayland_server::{Resource, Weak, protocol::wl_surface::WlSurface};

use crate::state::State;

/// The tab after (`forward`) or before the one at `at`, wrapping round;
/// the first (or last) when no tab is current.
pub fn step(len: usize, at: Option<usize>, forward: bool) -> Option<usize> {
    if len == 0 {
        return None;
    }
    Some(match (at, forward) {
        (Some(i), true) => (i + 1) % len,
        (Some(i), false) => (i + len - 1) % len,
        (None, true) => 0,
        (None, false) => len - 1,
    })
}

/// The tab to focus when the one at `at` closes: the next one, or the one
/// before if it was the last.
pub fn neighbour(len: usize, at: usize) -> Option<usize> {
    if len <= 1 {
        None
    } else if at + 1 < len {
        Some(at + 1)
    } else {
        Some(at - 1)
    }
}

/// The surface of the window that had the keyboard before this one last
/// took it; weak, so windows that took it from each other can still go.
#[derive(Default)]
struct Opener(RefCell<Option<Weak<WlSurface>>>);

/// Remembers that `window` took the keyboard from `from`.
pub fn set_opener(window: &Window, from: &Window) {
    use smithay::wayland::seat::WaylandFocus;
    let Some(surface) = from.wl_surface() else {
        return;
    };
    window.user_data().insert_if_missing(Opener::default);
    if let Some(opener) = window.user_data().get::<Opener>() {
        *opener.0.borrow_mut() = Some(surface.downgrade());
    }
}

fn opener(window: &Window) -> Option<WlSurface> {
    window
        .user_data()
        .get::<Opener>()?
        .0
        .borrow()
        .as_ref()?
        .upgrade()
        .ok()
}

impl State {
    /// Alt+Tab and Alt+Shift+Tab: activates the next or previous tab.
    pub fn cycle_tabs(&mut self, forward: bool) {
        let order = self.toplevels.order();
        let focused = self.focused_window().and_then(|w| {
            smithay::wayland::seat::WaylandFocus::wl_surface(&w).map(|s| s.into_owned())
        });
        let at = focused.and_then(|f| order.iter().position(|s| *s == f));
        if let Some(next) = step(order.len(), at, forward) {
            self.activate_toplevel(&order[next]);
        }
    }

    /// When the window `closing` goes: if it had the keyboard, gives it back
    /// to the window it came from, if that's still open; else focuses its
    /// neighbouring tab (or the top window, if it wasn't in the tabs yet).
    /// Minimised windows stay minimised (ADR-0084): they're passed over.
    pub fn focus_after_close(&mut self, closing: &WlSurface) {
        let focus = self.seat.get_keyboard().and_then(|k| k.current_focus());
        let had_focus = focus.as_ref().is_none_or(|f| {
            use smithay::utils::IsAlive;
            f.is(closing) || !f.alive()
        });
        if !had_focus {
            return;
        }
        let back = self
            .window_for(closing)
            .and_then(|w| opener(&w))
            .and_then(|s| self.window_for(&s))
            .filter(|o| self.space.elements().any(|e| e == o));
        if let Some(window) = back {
            self.focus_window(window);
            return;
        }
        let order: Vec<WlSurface> = self
            .toplevels
            .order()
            .into_iter()
            .filter(|s| s == closing || self.window_for(s).is_some_and(|w| !self.is_minimized(&w)))
            .collect();
        let next = order
            .iter()
            .position(|s| s == closing)
            .and_then(|at| neighbour(order.len(), at))
            .map(|i| order[i].clone())
            .filter(|s| self.window_for(s).is_some());
        eprintln!(
            "patina-compositor: {} closed with the keyboard; focusing the next tab",
            crate::toplevels::name(closing)
        );
        match next {
            Some(surface) => self.activate_toplevel(&surface),
            None => self.focus_top_window(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alt_tab_steps_along_the_tabs_and_wraps() {
        assert_eq!(step(3, Some(0), true), Some(1));
        assert_eq!(step(3, Some(2), true), Some(0));
        assert_eq!(step(3, Some(0), false), Some(2));
        assert_eq!(step(3, None, true), Some(0));
        assert_eq!(step(3, None, false), Some(2));
        assert_eq!(step(0, None, true), None);
    }

    #[test]
    fn closing_focuses_the_next_tab_or_the_one_before_the_last() {
        assert_eq!(neighbour(3, 0), Some(1));
        assert_eq!(neighbour(3, 1), Some(2));
        assert_eq!(neighbour(3, 2), Some(1));
        assert_eq!(neighbour(1, 0), None);
    }
}
