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

    /// Forgets `window` if it was minimised, as it closes.
    pub fn forget_minimized(&mut self, window: &Window) {
        self.minimized.remove(window);
    }
}
