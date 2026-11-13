//! Minimising (ADR-0084): a minimised window is taken out of the space, so
//! it isn't drawn and the pointer passes it by, and kept here with where it
//! was. Its tab stays in the bar, which is where it went (ADR-0027), and
//! focusing it in any way (its tab, Alt+Tab, Super+N, patina-raise) puts it
//! back where it was, on top.

use std::time::Duration;

use smithay::{
    desktop::Window,
    reexports::calloop::timer::{TimeoutAction, Timer},
    utils::{Logical, Point},
};

use crate::state::State;

/// How often a minimised window is told it may draw a frame (ADR-0123). It
/// isn't drawn, so nothing would: and a toolkit like winit holds back every
/// redraw until the last frame callback arrives, so an app whose window is
/// minimised would never run again, and media controls, a track ending or
/// a download finishing would wait for it to come back.
const FRAME_EVERY: Duration = Duration::from_millis(500);

/// How far four fingers go on the touchpad, in logical pixels, to count as a
/// swipe toward the window list's edge or away from it.
const SWIPE_FAR: f64 = 60.0;

/// Which way a four-finger swipe went.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Swipe {
    /// Toward the edge the window list is on.
    Toward,
    /// Away from it.
    Away,
}

/// A four-finger swipe on the touchpad: toward the edge the window list is
/// on minimises every window, and away from it puts back the ones it
/// minimised. It acts once it's gone far enough, once for each swipe, so
/// the fingers can stay down.
pub struct AllSwipe {
    /// Four fingers are down.
    on: bool,
    /// Whether this swipe has acted.
    acted: bool,
    /// The step toward the window list's edge: (1, 0) is the right.
    toward: (f64, f64),
    x: f64,
    y: f64,
    /// The windows the last swipe toward the edge minimised, bottom to top.
    gathered: Vec<Window>,
}

impl Default for AllSwipe {
    fn default() -> Self {
        Self {
            on: false,
            acted: false,
            toward: (0.0, 1.0),
            x: 0.0,
            y: 0.0,
            gathered: Vec::new(),
        }
    }
}

impl AllSwipe {
    /// Four fingers down, with the window list on the edge `toward`.
    fn begin(&mut self, fingers: u32, enabled: bool, toward: (f64, f64)) {
        self.on = enabled && fingers == 4;
        self.acted = false;
        self.toward = toward;
        (self.x, self.y) = (0.0, 0.0);
    }

    /// The fingers moved `by`: the swipe's direction, once it's mostly along
    /// the way to the edge, or away from it, and far enough.
    fn moved(&mut self, by: (f64, f64)) -> Option<Swipe> {
        if !self.on || self.acted {
            return None;
        }
        self.x += by.0;
        self.y += by.1;
        let along = self.x * self.toward.0 + self.y * self.toward.1;
        let across = self.x * self.toward.1 + self.y * self.toward.0;
        if along.abs() < SWIPE_FAR || along.abs() < across.abs() * 2.0 {
            return None;
        }
        self.acted = true;
        Some(if along > 0.0 {
            Swipe::Toward
        } else {
            Swipe::Away
        })
    }
}

/// The minimised windows, and where each was.
#[derive(Default)]
pub struct Minimized(Vec<(Window, Point<i32, Logical>)>);

impl Minimized {
    pub fn windows(&self) -> impl Iterator<Item = &Window> {
        self.0.iter().map(|(w, _)| w)
    }

    pub fn contains(&self, window: &Window) -> bool {
        self.windows().any(|w| w == window)
    }

    /// Where `window` was when it was minimised, if it is.
    pub fn location(&self, window: &Window) -> Option<Point<i32, Logical>> {
        self.0.iter().find(|(w, _)| w == window).map(|(_, at)| *at)
    }

    /// Forgets `window`, closed while minimised; where it was, if it was.
    pub fn remove(&mut self, window: &Window) -> Option<Point<i32, Logical>> {
        let at = self.0.iter().position(|(w, _)| w == window)?;
        Some(self.0.remove(at).1)
    }
}

impl State {
    /// Starts telling minimised windows to draw, slowly; called once.
    pub fn start_minimized_frames(&mut self) {
        let _ = self
            .loop_handle
            .insert_source(Timer::from_duration(FRAME_EVERY), |_, _, state| {
                state.minimized_frames();
                TimeoutAction::ToDuration(FRAME_EVERY)
            });
    }

    /// A frame callback for each minimised window.
    fn minimized_frames(&mut self) {
        if self.minimized.0.is_empty() {
            return;
        }
        let time = self.start.elapsed();
        let output = self.output.clone();
        for window in self.minimized.windows() {
            window.send_frame(&output, time, Some(Duration::ZERO), |_, _| {
                Some(output.clone())
            });
        }
    }

    pub fn is_minimized(&self, window: &Window) -> bool {
        self.minimized.contains(window)
    }

    /// Minimises `window`, giving the keyboard to the window then on top if
    /// it had it. An X window is told it's iconic, which a Wine game takes
    /// as being minimised (ADR-0044).
    pub fn minimize(&mut self, window: &Window) {
        if self.is_minimized(window) {
            return;
        }
        let Some(location) = self.space.element_location(window) else {
            return;
        };
        let focused = self.focused_window().as_ref() == Some(window);
        self.start_ghost(window, location, false);
        self.space.unmap_elem(window);
        self.minimized.0.push((window.clone(), location));
        window.set_activated(false);
        if let Some(t) = window.toplevel() {
            t.send_pending_configure();
        }
        if let Some(x11) = window.x11_surface() {
            let _ = x11.set_suspended(true);
        }
        if focused {
            self.focus_top_window();
        }
        // Whatever is under the pointer now has it.
        if let Some(pointer) = self.seat.get_pointer() {
            self.pointer_motion(pointer.current_location(), 0);
        }
        self.needs_redraw = true;
    }

    /// Minimises the focused window (Super+M, as Cmd+M on macOS).
    pub fn minimize_focused(&mut self) {
        if let Some(window) = self.focused_window() {
            self.minimize(&window);
        }
    }

    /// Puts a minimised `window` back where it was, on top. Focusing does
    /// it, so this isn't called by itself.
    pub(crate) fn unminimize(&mut self, window: &Window) {
        let Some(location) = self.minimized.remove(window) else {
            return;
        };
        self.space.map_element(window.clone(), location, true);
        self.start_ghost(window, location, true);
        self.tell_x11_where(window, location);
        // The bar may have come or gone, or the gaps changed, meanwhile.
        self.relayout_maximized();
        self.needs_redraw = true;
    }

    /// Restores a minimised `window` by focusing it, as an X window or
    /// foreign-toplevel asking for it does.
    pub fn restore(&mut self, window: &Window) {
        if self.is_minimized(window) {
            self.focus_window(window.clone());
        }
    }

    /// Four fingers touched down on the touchpad.
    pub fn all_swipe_begin(&mut self, fingers: u32) {
        let enabled = self.config.touchpad.four_finger_swipe;
        let pointer = self
            .seat
            .get_pointer()
            .map(|p| p.current_location())
            .unwrap_or_default();
        let toward = self.window_list_direction(&self.output_under(pointer).clone());
        self.all_swipe.begin(fingers, enabled, toward);
    }

    /// Four fingers moved `delta`; far enough toward the window list's
    /// edge, every window is minimised, and far enough away from it, those
    /// come back.
    pub fn all_swipe_update(&mut self, delta: Point<f64, Logical>) {
        match self.all_swipe.moved((delta.x, delta.y)) {
            Some(Swipe::Toward) => self.minimize_all(),
            Some(Swipe::Away) => self.restore_all(),
            None => {}
        }
    }

    pub fn all_swipe_end(&mut self) {
        self.all_swipe.on = false;
    }

    /// Minimises every window, remembering them for [`Self::restore_all`].
    /// An X window that's a menu or tooltip isn't one to minimise.
    fn minimize_all(&mut self) {
        let windows: Vec<Window> = self
            .space
            .elements()
            .filter(|w| !w.x11_surface().is_some_and(|x| x.is_override_redirect()))
            .cloned()
            .collect();
        if windows.is_empty() {
            return;
        }
        for window in &windows {
            self.minimize(window);
        }
        self.all_swipe.gathered = windows;
    }

    /// Puts back the windows the last swipe down minimised, in the order
    /// they were stacked, so the one that was on top is again, with the
    /// keyboard. One that's been brought back or closed since is left as it is.
    fn restore_all(&mut self) {
        for window in std::mem::take(&mut self.all_swipe.gathered) {
            self.restore(&window);
        }
    }

    /// Forgets `window` if it was minimised, as it closes.
    pub fn forget_minimized(&mut self, window: &Window) {
        self.minimized.remove(window);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_fingers_far_enough_toward_or_away_swipe_once() {
        let mut swipe = AllSwipe::default();
        swipe.begin(4, true, (0.0, 1.0));
        assert_eq!(swipe.moved((0.0, 30.0)), None);
        assert_eq!(swipe.moved((0.0, 40.0)), Some(Swipe::Toward));
        assert_eq!(swipe.moved((0.0, 90.0)), None, "once for each swipe");
        swipe.begin(4, true, (0.0, 1.0));
        assert_eq!(swipe.moved((0.0, -80.0)), Some(Swipe::Away));
    }

    #[test]
    fn toward_is_the_way_the_window_list_lies() {
        let mut swipe = AllSwipe::default();
        swipe.begin(4, true, (1.0, 0.0));
        assert_eq!(
            swipe.moved((90.0, 10.0)),
            Some(Swipe::Toward),
            "right, for a list on the right"
        );
        swipe.begin(4, true, (1.0, 0.0));
        assert_eq!(swipe.moved((-90.0, 0.0)), Some(Swipe::Away));
        swipe.begin(4, true, (0.0, -1.0));
        assert_eq!(
            swipe.moved((0.0, -90.0)),
            Some(Swipe::Toward),
            "up, for a list at the top"
        );
        swipe.begin(4, true, (-1.0, 0.0));
        assert_eq!(
            swipe.moved((0.0, 200.0)),
            None,
            "down is across a list on the left"
        );
    }

    #[test]
    fn other_swipes_and_the_setting_off_do_nothing() {
        let mut swipe = AllSwipe::default();
        swipe.begin(3, true, (0.0, 1.0));
        assert_eq!(swipe.moved((0.0, 200.0)), None);
        swipe.begin(4, false, (0.0, 1.0));
        assert_eq!(swipe.moved((0.0, 200.0)), None);
        swipe.begin(4, true, (0.0, 1.0));
        assert_eq!(swipe.moved((150.0, 80.0)), None, "mostly sideways");
    }
}
