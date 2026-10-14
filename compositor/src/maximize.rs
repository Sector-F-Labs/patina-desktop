//! Maximising and tiling: a placed window fills the work area (the output
//! less the bar's zone) or its left or right half, and remembers where it
//! was so restoring puts it back (ADR-0014, ADR-0023). Dragging a placed
//! window restores it under the pointer, as GNOME and macOS do. Super with
//! the arrows or vim keys places the focused window as Ubuntu does.

use std::cell::RefCell;

use smithay::{
    desktop::{Window, layer_map_for_output},
    input::pointer::{Focus, GrabStartData, PointerHandle},
    reexports::wayland_protocols::xdg::shell::server::xdg_toplevel,
    utils::{Logical, Point, Rectangle, Serial, Size},
};

use crate::grabs::MoveGrab;
use crate::state::State;

/// How a window is placed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    Maximized,
    Left,
    Right,
}

/// A direction from Super with an arrow or a vim key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Snap {
    Up,
    Down,
    Left,
    Right,
}

/// A placed window's placement, and where it was before, kept on the
/// window.
#[derive(Default)]
struct Placed(RefCell<Option<(Placement, Rectangle<i32, Logical>)>>);

fn placed(window: &Window) -> &Placed {
    window.user_data().insert_if_missing(Placed::default);
    window.user_data().get::<Placed>().expect("inserted above")
}

pub fn placement(window: &Window) -> Option<Placement> {
    placed(window).0.borrow().map(|(p, _)| p)
}

pub fn is_maximized(window: &Window) -> bool {
    placement(window) == Some(Placement::Maximized)
}

/// The rectangle `placement` fills in `area`. Halves split the width, the
/// left one taking the odd pixel.
pub fn rect_for(placement: Placement, area: Rectangle<i32, Logical>) -> Rectangle<i32, Logical> {
    let left = (area.size.w + 1) / 2;
    match placement {
        Placement::Maximized => area,
        Placement::Left => Rectangle::new(area.loc, Size::from((left, area.size.h))),
        Placement::Right => Rectangle::new(
            (area.loc.x + left, area.loc.y).into(),
            Size::from((area.size.w - left, area.size.h)),
        ),
    }
}

/// What `snap` does to a window placed as `now`, Ubuntu's way: up
/// maximises; down restores; left and right tile to that half, or restore
/// from the other one.
pub fn after_snap(now: Option<Placement>, snap: Snap) -> Option<Placement> {
    use Placement::*;
    match (snap, now) {
        (Snap::Up, _) => Some(Maximized),
        (Snap::Down, _) => None,
        (Snap::Left, Some(Right)) | (Snap::Right, Some(Left)) => None,
        (Snap::Left, _) => Some(Left),
        (Snap::Right, _) => Some(Right),
    }
}

/// Where a restored window of `size` goes so the pointer, at `pointer`
/// over the placed window `rect`, stays at the same fraction of its width.
/// Its top stays at the placed window's top, so the pointer is just as
/// deep into the title bar.
pub fn restored_under_pointer(
    rect: Rectangle<i32, Logical>,
    size: (i32, i32),
    pointer: Point<f64, Logical>,
) -> Point<i32, Logical> {
    let fraction = ((pointer.x - rect.loc.x as f64) / rect.size.w.max(1) as f64).clamp(0.0, 1.0);
    let x = pointer.x - fraction * size.0 as f64;
    Point::from((x.round() as i32, rect.loc.y))
}

/// The xdg states a placement shows: maximised, or tiled on the sides
/// that touch the screen's edges, so apps can square those corners.
fn set_states(
    toplevel: &smithay::wayland::shell::xdg::ToplevelSurface,
    placement: Option<Placement>,
) {
    use xdg_toplevel::State as S;
    toplevel.with_pending_state(|s| {
        for state in [
            S::Maximized,
            S::TiledLeft,
            S::TiledRight,
            S::TiledTop,
            S::TiledBottom,
        ] {
            s.states.unset(state);
        }
        let set: &[S] = match placement {
            Some(Placement::Maximized) => &[S::Maximized],
            Some(Placement::Left) => &[S::TiledLeft, S::TiledTop, S::TiledBottom],
            Some(Placement::Right) => &[S::TiledRight, S::TiledTop, S::TiledBottom],
            None => &[],
        };
        for state in set {
            s.states.set(*state);
        }
    });
}

impl State {
    /// The area a maximised window fills.
    fn work_area(&self) -> Rectangle<i32, Logical> {
        layer_map_for_output(&self.output).non_exclusive_zone()
    }

    /// Places `window` as `placement`, remembering where it was the first
    /// time.
    pub fn place_as(&mut self, window: &Window, placement: Placement) {
        let Some(toplevel) = window.toplevel() else {
            return;
        };
        let area = self.work_area();
        let rect = rect_for(placement, area);
        let slot = placed(window);
        let before = match *slot.0.borrow() {
            Some((_, before)) => before,
            None => Rectangle::new(
                self.space.element_location(window).unwrap_or(area.loc),
                window.geometry().size,
            ),
        };
        *slot.0.borrow_mut() = Some((placement, before));
        set_states(toplevel, Some(placement));
        toplevel.with_pending_state(|s| s.size = Some(rect.size));
        if toplevel.is_initial_configure_sent() {
            toplevel.send_pending_configure();
        }
        self.place(window, rect.loc);
        self.needs_redraw = true;
    }

    /// Asks `window` to fill the work area.
    pub fn maximize(&mut self, window: &Window) {
        self.place_as(window, Placement::Maximized);
    }

    /// Puts `window` back where it was before it was placed; `at` overrides
    /// where (for a drag).
    pub fn unmaximize(&mut self, window: &Window, at: Option<Point<i32, Logical>>) {
        let Some(toplevel) = window.toplevel() else {
            return;
        };
        let Some((_, before)) = placed(window).0.borrow_mut().take() else {
            return;
        };
        set_states(toplevel, None);
        toplevel.with_pending_state(|s| {
            s.size = (before.size.w > 0 && before.size.h > 0).then_some(before.size);
        });
        if toplevel.is_initial_configure_sent() {
            toplevel.send_pending_configure();
        }
        self.place(window, at.unwrap_or(before.loc));
        self.needs_redraw = true;
    }

    /// Super with an arrow or vim key, on the focused window.
    pub fn snap(&mut self, snap: Snap) {
        let Some(window) = self.focused_window() else {
            return;
        };
        match after_snap(placement(&window), snap) {
            Some(p) => self.place_as(&window, p),
            None => self.unmaximize(&window, None),
        }
    }

    /// Keeps placed windows filling their part of the work area after it
    /// changed: the output resized or rescaled, or the bar came or went.
    pub fn relayout_maximized(&mut self) {
        let area = self.work_area();
        let placed: Vec<(Window, Placement)> = self
            .space
            .elements()
            .filter_map(|w| placement(w).map(|p| (w.clone(), p)))
            .collect();
        for (window, placement) in placed {
            // Called on every commit of a layer surface (the bar redraws
            // each second), so only act on a real change.
            let Some(toplevel) = window.toplevel() else {
                continue;
            };
            let rect = rect_for(placement, area);
            if toplevel.with_pending_state(|s| s.size) != Some(rect.size) {
                toplevel.with_pending_state(|s| s.size = Some(rect.size));
                if toplevel.is_initial_configure_sent() {
                    toplevel.send_pending_configure();
                }
            }
            self.place(&window, rect.loc);
        }
    }

    /// Starts dragging `window`. A placed window is restored first, under
    /// the pointer.
    pub fn start_move(
        &mut self,
        window: Window,
        start: GrabStartData<State>,
        pointer: PointerHandle<State>,
        serial: Serial,
    ) {
        let was = *placed(&window).0.borrow();
        if let Some((placement, before)) = was {
            let rect = rect_for(placement, self.work_area());
            let at = restored_under_pointer(
                rect,
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

    fn area() -> Rectangle<i32, Logical> {
        Rectangle::new((0, 0).into(), (1281, 759).into())
    }

    #[test]
    fn halves_split_the_work_area() {
        let left = rect_for(Placement::Left, area());
        let right = rect_for(Placement::Right, area());
        assert_eq!(left, Rectangle::new((0, 0).into(), (641, 759).into()));
        assert_eq!(right, Rectangle::new((641, 0).into(), (640, 759).into()));
        assert_eq!(rect_for(Placement::Maximized, area()), area());
    }

    #[test]
    fn snapping_follows_ubuntu() {
        use Placement::*;
        assert_eq!(after_snap(None, Snap::Up), Some(Maximized));
        assert_eq!(after_snap(Some(Left), Snap::Up), Some(Maximized));
        assert_eq!(after_snap(Some(Maximized), Snap::Down), None);
        assert_eq!(after_snap(Some(Left), Snap::Down), None);
        assert_eq!(after_snap(None, Snap::Left), Some(Left));
        assert_eq!(after_snap(Some(Maximized), Snap::Left), Some(Left));
        assert_eq!(
            after_snap(Some(Right), Snap::Left),
            None,
            "back from the other half"
        );
        assert_eq!(after_snap(Some(Left), Snap::Right), None);
        assert_eq!(after_snap(Some(Left), Snap::Left), Some(Left));
    }

    #[test]
    fn a_restored_window_keeps_the_pointer_at_the_same_fraction() {
        let rect = Rectangle::new((0, 0).into(), (1000, 700).into());
        // Grabbed three quarters of the way along a maximised title bar.
        let at = restored_under_pointer(rect, (400, 300), (750.0, 10.0).into());
        assert_eq!(at, Point::from((450, 0)), "750 - 0.75 * 400");
    }

    #[test]
    fn the_pointer_stays_on_the_restored_title_bar() {
        let rect = Rectangle::new((0, 41).into(), (1000, 700).into());
        let at = restored_under_pointer(rect, (400, 300), (0.0, 55.0).into());
        assert_eq!(at, Point::from((0, 41)));
    }
}
