//! Maximising: a maximised window fills the area the layer surfaces (the
//! bar) leave free, and remembers where it was so unmaximising puts it
//! back. Dragging a maximised window restores it under the pointer, as
//! GNOME and macOS do (ADR-0014).

use std::cell::RefCell;

use smithay::{
    desktop::{Window, layer_map_for_output},
    input::pointer::{Focus, GrabStartData, PointerHandle},
    reexports::wayland_protocols::xdg::shell::server::xdg_toplevel,
    utils::{Logical, Point, Rectangle, Serial},
};

use crate::grabs::MoveGrab;
use crate::state::State;

/// Where a maximised window was before, kept on the window.
#[derive(Default)]
struct Restore(RefCell<Option<Rectangle<i32, Logical>>>);

fn restore_of(window: &Window) -> &Restore {
    window.user_data().insert_if_missing(Restore::default);
    window.user_data().get::<Restore>().expect("inserted above")
}

pub fn is_maximized(window: &Window) -> bool {
    restore_of(window).0.borrow().is_some()
}

/// Where a restored window of `size` goes so the pointer, at `pointer`
/// over the maximised window `zone`, stays at the same fraction of its
/// width. Its top stays at the work area's top, so the pointer is just as
/// deep into the title bar.
pub fn restored_under_pointer(
    zone: Rectangle<i32, Logical>,
    size: (i32, i32),
    pointer: Point<f64, Logical>,
) -> Point<i32, Logical> {
    let fraction = ((pointer.x - zone.loc.x as f64) / zone.size.w.max(1) as f64).clamp(0.0, 1.0);
    let x = pointer.x - fraction * size.0 as f64;
    Point::from((x.round() as i32, zone.loc.y))
}

impl State {
    /// The area a maximised window fills.
    fn work_area(&self) -> Rectangle<i32, Logical> {
        layer_map_for_output(&self.output).non_exclusive_zone()
    }

    /// Asks `window` to fill the work area, remembering where it was.
    pub fn maximize(&mut self, window: &Window) {
        let Some(toplevel) = window.toplevel() else {
            return;
        };
        let zone = self.work_area();
        let restore = restore_of(window);
        if restore.0.borrow().is_none() {
            let location = self.space.element_location(window).unwrap_or(zone.loc);
            *restore.0.borrow_mut() = Some(Rectangle::new(location, window.geometry().size));
        }
        toplevel.with_pending_state(|s| {
            s.states.set(xdg_toplevel::State::Maximized);
            s.size = Some(zone.size);
        });
        if toplevel.is_initial_configure_sent() {
            toplevel.send_pending_configure();
        }
        self.place(window, zone.loc);
        self.needs_redraw = true;
    }

    /// Puts `window` back where it was before it was maximised; `at`
    /// overrides where (for a drag).
    pub fn unmaximize(&mut self, window: &Window, at: Option<Point<i32, Logical>>) {
        let Some(toplevel) = window.toplevel() else {
            return;
        };
        let Some(before) = restore_of(window).0.borrow_mut().take() else {
            return;
        };
        toplevel.with_pending_state(|s| {
            s.states.unset(xdg_toplevel::State::Maximized);
            s.size = (before.size.w > 0 && before.size.h > 0).then_some(before.size);
        });
        if toplevel.is_initial_configure_sent() {
            toplevel.send_pending_configure();
        }
        self.place(window, at.unwrap_or(before.loc));
        self.needs_redraw = true;
    }

    /// Keeps maximised windows filling the work area after it changed: the
    /// output resized, or the bar came or went.
    pub fn relayout_maximized(&mut self) {
        let zone = self.work_area();
        let maximized: Vec<Window> = self
            .space
            .elements()
            .filter(|w| is_maximized(w))
            .cloned()
            .collect();
        for window in maximized {
            // Called on every commit of a layer surface (the bar redraws
            // each second), so only act on a real change.
            let Some(toplevel) = window.toplevel() else {
                continue;
            };
            if toplevel.with_pending_state(|s| s.size) != Some(zone.size) {
                toplevel.with_pending_state(|s| s.size = Some(zone.size));
                if toplevel.is_initial_configure_sent() {
                    toplevel.send_pending_configure();
                }
            }
            self.place(&window, zone.loc);
        }
    }

    /// Starts dragging `window`. A maximised window is restored first,
    /// under the pointer.
    pub fn start_move(
        &mut self,
        window: Window,
        start: GrabStartData<State>,
        pointer: PointerHandle<State>,
        serial: Serial,
    ) {
        if is_maximized(&window) {
            let before = restore_of(&window).0.borrow().unwrap_or_default();
            let at = restored_under_pointer(
                self.work_area(),
                (before.size.w, before.size.h),
                pointer.current_location(),
            );
            self.unmaximize(&window, Some(at));
        }
        let Some(initial) = self.space.element_location(&window) else {
            return;
        };
        pointer.set_grab(
            self,
            MoveGrab {
                start,
                window,
                initial,
            },
            serial,
            Focus::Clear,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_restored_window_keeps_the_pointer_at_the_same_fraction() {
        let zone = Rectangle::new((0, 0).into(), (1000, 700).into());
        // Grabbed three quarters of the way along a maximised title bar.
        let at = restored_under_pointer(zone, (400, 300), (750.0, 10.0).into());
        assert_eq!(at, Point::from((450, 0)), "750 - 0.75 * 400");
    }

    #[test]
    fn the_pointer_stays_on_the_restored_title_bar() {
        let zone = Rectangle::new((0, 41).into(), (1000, 700).into());
        let at = restored_under_pointer(zone, (400, 300), (0.0, 55.0).into());
        assert_eq!(at, Point::from((0, 41)));
    }
}
