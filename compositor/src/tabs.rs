//! Moving between windows: the bar's tabs are the one list of open windows
//! (ADR-0027). Alt+Tab goes by most recent use, as on other desktops
//! (ADR-0127), and closing a window gives the keyboard back to the window it
//! came from, or else its neighbouring tab, as in a browser (ADR-0059).
//! Pinned apps come first in the bar (ADR-0080), and the bar can move a
//! window along (`dbus`).

pub mod dbus;

use std::cell::RefCell;

use smithay::desktop::Window;
use smithay::reexports::wayland_server::{Resource, Weak, protocol::wl_surface::WlSurface};

use crate::state::State;

/// Windows in the order they were last used, and the Alt+Tab round in
/// progress (ADR-0127). A quick Alt+Tab goes to the window used before this
/// one, and again goes back, as on other desktops; holding Alt and pressing
/// Tab again goes further back, and Shift goes round the other way. The order
/// only changes when Alt is let go, so a round doesn't shuffle what it walks.
pub struct Recent<T> {
    used: Vec<T>,
    /// The order the round walks (most recently used first, so the window
    /// the round started on is first) and where it is in it.
    round: Option<(Vec<T>, usize)>,
}

impl<T> Default for Recent<T> {
    fn default() -> Self {
        Self {
            used: Vec::new(),
            round: None,
        }
    }
}

impl<T: Clone + PartialEq> Recent<T> {
    /// `window` has the keyboard now: it is the most recently used. Not
    /// during a round, which puts its choice first when it ends.
    pub fn used(&mut self, window: &T) {
        if self.round.is_none() {
            self.used.retain(|w| w != window);
            self.used.insert(0, window.clone());
        }
    }

    /// One Alt+Tab (or Alt+Shift+Tab) press: the window to go to. `tabs` is
    /// every open window in the bar's order; ones not used yet follow the
    /// used ones, in that order.
    pub fn cycle(&mut self, tabs: &[T], forward: bool) -> Option<T> {
        if self.round.is_none() {
            self.used.retain(|w| tabs.contains(w));
            let mut order = self.used.clone();
            let unused: Vec<T> = tabs.iter().filter(|t| !order.contains(t)).cloned().collect();
            order.extend(unused);
            self.round = Some((order, 0));
        }
        let (order, at) = self.round.as_mut()?;
        let len = order.len();
        if len == 0 {
            return None;
        }
        *at = if forward { (*at + 1) % len } else { (*at + len - 1) % len };
        Some(order[*at].clone())
    }

    /// Alt is up: the round is over, and where it ended is the most
    /// recently used.
    pub fn end(&mut self) {
        if let Some((order, at)) = self.round.take() {
            self.used(&order[at]);
        }
    }
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
    /// Alt+Tab and Alt+Shift+Tab: activates the window used before this one,
    /// or, with Alt held, the one before that (ADR-0127).
    pub fn cycle_tabs(&mut self, forward: bool) {
        let order = self.toplevels.order();
        let focused = self.focused_window().and_then(|w| {
            smithay::wayland::seat::WaylandFocus::wl_surface(&w).map(|s| s.into_owned())
        });
        if let Some(next) = self.recent.cycle(&order, forward) {
            // A fullscreen window with no other tab to go to: step out of it
            // to the desktop, and the next Alt+Tab brings it back (ADR-0103).
            if focused.as_ref() == Some(&next) && self.fullscreen_on_top().is_some() {
                self.minimize_focused();
                return;
            }
            self.activate_toplevel(&next);
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

    /// A used order of a, b, c (c most recent) and a round with Alt held.
    fn used(order: &[char]) -> Recent<char> {
        let mut recent = Recent::default();
        for w in order.iter().rev() {
            recent.used(w);
        }
        recent
    }

    #[test]
    fn a_quick_alt_tab_swaps_with_the_previous_window_and_back() {
        let tabs = ['a', 'b', 'c'];
        // c has the keyboard, b was before it.
        let mut r = used(&['c', 'b', 'a']);
        assert_eq!(r.cycle(&tabs, true), Some('b'));
        r.used(&'b');
        r.end();
        // Again: back to c, not on to a.
        assert_eq!(r.cycle(&tabs, true), Some('c'));
        r.end();
        assert_eq!(r.cycle(&tabs, true), Some('b'));
    }

    #[test]
    fn holding_alt_walks_further_back_and_shift_goes_the_other_way() {
        let tabs = ['a', 'b', 'c'];
        let mut r = used(&['c', 'b', 'a']);
        assert_eq!(r.cycle(&tabs, true), Some('b'));
        assert_eq!(r.cycle(&tabs, true), Some('a'));
        // Past the oldest, round to the one we started on.
        assert_eq!(r.cycle(&tabs, true), Some('c'));
        assert_eq!(r.cycle(&tabs, false), Some('a'));
        r.end();
        // Where it ended is now the most recent; c is next.
        assert_eq!(r.cycle(&tabs, true), Some('c'));
        // Shift from the start goes to the least recently used.
        r.end();
        let mut r = used(&['c', 'b', 'a']);
        assert_eq!(r.cycle(&tabs, false), Some('a'));
    }

    #[test]
    fn windows_never_used_follow_in_tab_order_and_closed_ones_drop_out() {
        let mut r = used(&['c']);
        // b and a were never focused; the bar lists a, b, c.
        assert_eq!(r.cycle(&['a', 'b', 'c'], true), Some('a'));
        assert_eq!(r.cycle(&['a', 'b', 'c'], true), Some('b'));
        r.end();
        // c closed.
        let mut r = used(&['c', 'b']);
        assert_eq!(r.cycle(&['a', 'b'], true), Some('a'));
        // One window: nowhere else to go.
        let mut r = used(&['a']);
        assert_eq!(r.cycle(&['a'], true), Some('a'));
        assert_eq!(Recent::<char>::default().cycle(&[], true), None);
    }

    #[test]
    fn closing_focuses_the_next_tab_or_the_one_before_the_last() {
        assert_eq!(neighbour(3, 0), Some(1));
        assert_eq!(neighbour(3, 1), Some(2));
        assert_eq!(neighbour(3, 2), Some(1));
        assert_eq!(neighbour(1, 0), None);
    }
}
