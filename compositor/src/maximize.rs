//! Placing windows: maximised at the app's request, or filling or tiled
//! where the tiler says (ADR-0014, ADR-0049, ADR-0053), each remembering
//! where it was so restoring puts it back. Dragging a placed window restores
//! it under the pointer, as GNOME and macOS do. Super with the arrows or vim
//! keys goes to the tiler; with Shift, it grows and shrinks the window here.

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
    /// Maximised: the app asked (its maximise button, or a double click on
    /// its title bar). It fills the work area, gaps or not, and is told
    /// it's maximised (ADR-0052).
    Maximized,
    /// Tiled upward (dragged to the top, or Super+Up): it fills the area
    /// inside the gaps as a tiled window, not a maximised one.
    Fill,
    /// Wherever the tiler put it (ADR-0049).
    Rect(Rectangle<i32, Logical>),
}

/// Resizing around the centre (Super+Shift+K/J/L/H).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resize {
    Grow,
    Shrink,
    Wider,
    Narrower,
}

/// The smallest a window gets by Super+Shift+J or H, whatever the app allows.
const SMALLEST: (i32, i32) = (120, 80);

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

/// Where a placed window was before it was placed, or `None` if it floats.
pub fn before(window: &Window) -> Option<Rectangle<i32, Logical>> {
    placed(window).0.borrow().map(|(_, before)| before)
}

pub fn is_maximized(window: &Window) -> bool {
    placement(window) == Some(Placement::Maximized)
}

/// The rectangle `placement` fills: maximised, the `work_area`; filling,
/// the `tiling_area` inside the gaps; tiled, where the tiler put it.
pub fn rect_for(
    placement: Placement,
    work_area: Rectangle<i32, Logical>,
    tiling_area: Rectangle<i32, Logical>,
) -> Rectangle<i32, Logical> {
    match placement {
        Placement::Maximized => work_area,
        Placement::Fill => tiling_area,
        Placement::Rect(rect) => rect,
    }
}

/// `area` less `gap` all round: where tiled windows go.
pub fn inset(area: Rectangle<i32, Logical>, gap: i32) -> Rectangle<i32, Logical> {
    let gap = gap.min(area.size.w / 4).min(area.size.h / 4).max(0);
    Rectangle::new(
        (area.loc.x + gap, area.loc.y + gap).into(),
        Size::from((area.size.w - 2 * gap, area.size.h - 2 * gap)),
    )
}

/// `rect` grown by `step` toward `toward`: that edge moves out, the
/// opposite one stays. It stops at the edge of `area`, never shrinks, and
/// stays within `max` (0 for no limit).
pub fn grown(
    rect: Rectangle<i32, Logical>,
    area: Rectangle<i32, Logical>,
    toward: Snap,
    step: i32,
    max: Size<i32, Logical>,
) -> Rectangle<i32, Logical> {
    let (mut left, mut top) = (rect.loc.x, rect.loc.y);
    let (mut right, mut bottom) = (left + rect.size.w, top + rect.size.h);
    let area_right = area.loc.x + area.size.w;
    let area_bottom = area.loc.y + area.size.h;
    match toward {
        Snap::Right => right = (right + step).min(area_right).max(right),
        Snap::Left => left = (left - step).max(area.loc.x).min(left),
        Snap::Down => bottom = (bottom + step).min(area_bottom).max(bottom),
        Snap::Up => top = (top - step).max(area.loc.y).min(top),
    }
    let (max_w, max_h) = (max.w, max.h);
    if max_w > 0 && right - left > max_w {
        match toward {
            Snap::Left => left = right - max_w,
            _ => right = left + max_w,
        }
    }
    if max_h > 0 && bottom - top > max_h {
        match toward {
            Snap::Up => top = bottom - max_h,
            _ => bottom = top + max_h,
        }
    }
    Rectangle::new((left, top).into(), Size::from((right - left, bottom - top)))
}

/// `rect` resized by (`dw`, `dh`) around its centre, kept between `min`
/// and `max` (0 for no limit, and never past `area`), and moved back inside
/// `area` if it sticks out.
pub fn resized_around_centre(
    rect: Rectangle<i32, Logical>,
    area: Rectangle<i32, Logical>,
    dw: i32,
    dh: i32,
    min: Size<i32, Logical>,
    max: Size<i32, Logical>,
) -> Rectangle<i32, Logical> {
    let limit = |v: i32, lo: i32, hi: i32, room: i32| {
        let hi = if hi > 0 { hi.min(room) } else { room };
        v.max(lo).min(hi.max(lo))
    };
    let (min_w, min_h) = (min.w.max(SMALLEST.0), min.h.max(SMALLEST.1));
    let (max_w, max_h) = (max.w, max.h);
    let w = limit(rect.size.w + dw, min_w, max_w, area.size.w);
    let h = limit(rect.size.h + dh, min_h, max_h, area.size.h);
    let centre = (rect.loc.x + rect.size.w / 2, rect.loc.y + rect.size.h / 2);
    let x = (centre.0 - w / 2).clamp(area.loc.x, (area.loc.x + area.size.w - w).max(area.loc.x));
    let y = (centre.1 - h / 2).clamp(area.loc.y, (area.loc.y + area.size.h - h).max(area.loc.y));
    Rectangle::new((x, y).into(), Size::from((w, h)))
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

/// The xdg states a window placed as `placement` at `rect` shows:
/// maximised only if it is, otherwise tiled on the sides that touch the
/// work area's edges, `area`, so apps can square those corners. With gaps,
/// none touch, and a tiled window keeps its rounded corners.
pub fn states_for(
    placement: Placement,
    rect: Rectangle<i32, Logical>,
    area: Rectangle<i32, Logical>,
) -> Vec<xdg_toplevel::State> {
    use xdg_toplevel::State as S;
    if placement == Placement::Maximized {
        return vec![S::Maximized];
    }
    [
        (rect.loc.x == area.loc.x, S::TiledLeft),
        (
            rect.loc.x + rect.size.w == area.loc.x + area.size.w,
            S::TiledRight,
        ),
        (rect.loc.y == area.loc.y, S::TiledTop),
        (
            rect.loc.y + rect.size.h == area.loc.y + area.size.h,
            S::TiledBottom,
        ),
    ]
    .into_iter()
    .filter_map(|(touches, state)| touches.then_some(state))
    .collect()
}

/// Shows `placement` at `rect` of the work area `area` in `toplevel`'s
/// xdg states (see `states_for`), or none for a floating window.
fn set_states(
    toplevel: &smithay::wayland::shell::xdg::ToplevelSurface,
    placed: Option<(Placement, Rectangle<i32, Logical>)>,
    area: Rectangle<i32, Logical>,
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
        if let Some((placement, rect)) = placed {
            for state in states_for(placement, rect, area) {
                s.states.set(state);
            }
        }
    });
}

impl State {
    /// The area a maximised window fills.
    pub fn work_area(&self) -> Rectangle<i32, Logical> {
        layer_map_for_output(&self.output).non_exclusive_zone()
    }

    /// The area tiled windows fill: the work area less the gaps, if
    /// they're on. Maximised windows fill the work area itself.
    pub fn tiling_area(&self) -> Rectangle<i32, Logical> {
        inset(self.work_area(), self.config.gap())
    }

    /// Where `placement` puts a window now.
    pub fn placed_rect(&self, placement: Placement) -> Rectangle<i32, Logical> {
        rect_for(placement, self.work_area(), self.tiling_area())
    }

    /// Places `window` as `placement`, remembering where it was the first
    /// time.
    pub fn place_as(&mut self, window: &Window, placement: Placement) {
        let Some(toplevel) = window.toplevel() else {
            return;
        };
        let area = self.work_area();
        let rect = self.placed_rect(placement);
        let slot = placed(window);
        let before = match *slot.0.borrow() {
            Some((_, before)) => before,
            None => Rectangle::new(
                self.space.element_location(window).unwrap_or(area.loc),
                window.geometry().size,
            ),
        };
        *slot.0.borrow_mut() = Some((placement, before));
        set_states(toplevel, Some((placement, rect)), area);
        toplevel.with_pending_state(|s| s.size = Some(rect.size));
        if toplevel.is_initial_configure_sent() {
            toplevel.send_pending_configure();
        }
        self.place(window, rect.loc);
        self.needs_redraw = true;
    }

    /// Moves tiled `window` to `rect`, as dragging the line beside it does
    /// (ADR-0054); `resizing` while the line moves. Where it was before
    /// it was tiled stays remembered.
    pub fn retile(&mut self, window: &Window, rect: Rectangle<i32, Logical>, resizing: bool) {
        let Some(toplevel) = window.toplevel() else {
            return;
        };
        let slot = placed(window);
        let Some((_, before)) = *slot.0.borrow() else {
            return;
        };
        *slot.0.borrow_mut() = Some((Placement::Rect(rect), before));
        set_states(
            toplevel,
            Some((Placement::Rect(rect), rect)),
            self.work_area(),
        );
        crate::split::set_resizing(window, resizing);
        toplevel.with_pending_state(|s| s.size = Some(rect.size));
        if toplevel.is_initial_configure_sent() {
            toplevel.send_pending_configure();
        }
        self.place(window, rect.loc);
        self.needs_redraw = true;
    }

    /// Asks `window` to fill the work area, maximised: only for the app's
    /// own request.
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
        set_states(toplevel, None, Rectangle::default());
        toplevel.with_pending_state(|s| {
            s.size = (before.size.w > 0 && before.size.h > 0).then_some(before.size);
        });
        if toplevel.is_initial_configure_sent() {
            toplevel.send_pending_configure();
        }
        self.place(window, at.unwrap_or(before.loc));
        self.needs_redraw = true;
    }

    /// Super with an arrow or vim key, on the focused window: the tiler
    /// decides what it does (ADR-0053).
    pub fn snap(&mut self, snap: Snap) {
        let Some(window) = self.focused_window() else {
            return;
        };
        self.tiling.snap_requested(&window, snap);
    }

    /// Grows the focused window toward `toward` by a tenth of the work area
    /// (Super+Shift with an arrow or vim key). A tiled window becomes a
    /// floating one at its size first; a maximised one has nowhere to grow.
    pub fn grow(&mut self, toward: Snap) {
        let Some(window) = self.focused_window() else {
            return;
        };
        let Some(toplevel) = window.toplevel().cloned() else {
            return;
        };
        match placement(&window) {
            Some(Placement::Maximized) => return,
            Some(_) => {
                *placed(&window).0.borrow_mut() = None;
                set_states(&toplevel, None, Rectangle::default());
            }
            None => {}
        }
        let area = self.work_area();
        let Some(location) = self.space.element_location(&window) else {
            return;
        };
        let rect = Rectangle::new(location, window.geometry().size);
        let step = match toward {
            Snap::Left | Snap::Right => area.size.w / 10,
            Snap::Up | Snap::Down => area.size.h / 10,
        }
        .max(32);
        let max = smithay::wayland::compositor::with_states(toplevel.wl_surface(), |states| {
            states
                .cached_state
                .get::<smithay::wayland::shell::xdg::SurfaceCachedState>()
                .current()
                .max_size
        });
        let new = grown(rect, area, toward, step, max);
        if new == rect {
            return;
        }
        let edges = crate::grabs::Edges {
            left: toward == Snap::Left,
            right: toward == Snap::Right,
            top: toward == Snap::Up,
            bottom: toward == Snap::Down,
        };
        crate::grabs::resize_keeping(&window, edges, rect, new.size);
        self.needs_redraw = true;
    }

    /// Resizes the focused window around its centre by a tenth of the work
    /// area (Super+Shift+K/J/L/H). Like growing, a tiled window becomes a
    /// floating one first, and a maximised one is left alone.
    pub fn resize_focused(&mut self, how: Resize) {
        let Some(window) = self.focused_window() else {
            return;
        };
        let Some(toplevel) = window.toplevel().cloned() else {
            return;
        };
        match placement(&window) {
            Some(Placement::Maximized) => return,
            Some(_) => {
                *placed(&window).0.borrow_mut() = None;
                set_states(&toplevel, None, Rectangle::default());
            }
            None => {}
        }
        let area = self.work_area();
        let Some(location) = self.space.element_location(&window) else {
            return;
        };
        let rect = Rectangle::new(location, window.geometry().size);
        let (step_w, step_h) = ((area.size.w / 10).max(32), (area.size.h / 10).max(32));
        let (dw, dh) = match how {
            Resize::Grow => (step_w, step_h),
            Resize::Shrink => (-step_w, -step_h),
            Resize::Wider => (step_w, 0),
            Resize::Narrower => (-step_w, 0),
        };
        let (min, max) =
            smithay::wayland::compositor::with_states(toplevel.wl_surface(), |states| {
                let mut cached = states
                    .cached_state
                    .get::<smithay::wayland::shell::xdg::SurfaceCachedState>();
                let current = cached.current();
                (current.min_size, current.max_size)
            });
        let new = resized_around_centre(rect, area, dw, dh, min, max);
        if new == rect {
            return;
        }
        // Both edges of an axis that changes move: the window stays centred
        // as the app resizes.
        let edges = crate::grabs::Edges {
            left: new.size.w != rect.size.w,
            right: new.size.w != rect.size.w,
            top: new.size.h != rect.size.h,
            bottom: new.size.h != rect.size.h,
        };
        crate::grabs::resize_keeping(&window, edges, rect, new.size);
        // Moved back inside the work area if it would stick out.
        let centred = Point::from((
            rect.loc.x + (rect.size.w - new.size.w) / 2,
            rect.loc.y + (rect.size.h - new.size.h) / 2,
        ));
        if new.loc != centred {
            crate::grabs::shift_resize(&window, new.loc - centred);
        }
        self.needs_redraw = true;
    }

    /// Keeps placed windows filling their part of the work area after it
    /// changed: the output resized or rescaled, or the bar came or went.
    pub fn relayout_maximized(&mut self) {
        self.announce_work_area(self.tiling_area());
        let work_area = self.work_area();
        let placed: Vec<(Window, Placement)> = self
            .space
            .elements()
            .filter_map(|w| placement(w).map(|p| (w.clone(), p)))
            .collect();
        for (window, placement) in placed {
            // Called on every commit of a layer surface (the bar redraws
            // each second), so only a real change is sent: a new size, or
            // new tiled sides after the gaps changed.
            let Some(toplevel) = window.toplevel() else {
                continue;
            };
            let rect = self.placed_rect(placement);
            set_states(toplevel, Some((placement, rect)), work_area);
            toplevel.with_pending_state(|s| s.size = Some(rect.size));
            if toplevel.is_initial_configure_sent() {
                toplevel.send_pending_configure();
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
            let rect = self.placed_rect(placement);
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
        self.drag_started(&window);
        // The arrow while a window moves, as on macOS, whatever the title
        // bar under it last asked for.
        self.grab_cursor = Some(smithay::input::pointer::CursorIcon::Default);
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
    fn placements_fill_their_area() {
        let tiled = inset(area(), 8);
        assert_eq!(tiled, Rectangle::new((8, 8).into(), (1265, 743).into()));
        assert_eq!(rect_for(Placement::Maximized, area(), tiled), area());
        assert_eq!(rect_for(Placement::Fill, area(), tiled), tiled);
        let half = Rectangle::new((8, 8).into(), (629, 743).into());
        assert_eq!(rect_for(Placement::Rect(half), area(), tiled), half);
        // Off the work area's edges, they keep their rounded corners: not
        // tiled, and filling isn't maximising.
        assert!(states_for(Placement::Rect(half), half, area()).is_empty());
        assert!(states_for(Placement::Fill, tiled, area()).is_empty());
        // A gap never eats a small screen.
        assert_eq!(
            inset(Rectangle::new((0, 0).into(), (100, 80).into()), 64)
                .size
                .w,
            60
        );
    }

    #[test]
    fn apps_are_told_which_sides_touch_the_work_area() {
        use xdg_toplevel::State as S;
        let area = Rectangle::new((0, 40).into(), (1200, 760).into());
        assert_eq!(states_for(Placement::Maximized, area, area), [S::Maximized]);
        // Filled without gaps, it's tiled on every side, not maximised.
        assert_eq!(
            states_for(Placement::Fill, area, area),
            [S::TiledLeft, S::TiledRight, S::TiledTop, S::TiledBottom]
        );
        let left = Rectangle::new((0, 40).into(), (600, 760).into());
        assert_eq!(
            states_for(Placement::Rect(left), left, area),
            [S::TiledLeft, S::TiledTop, S::TiledBottom]
        );
        // A tiler's middle third touches only the top and bottom.
        let middle = Rectangle::new((400, 40).into(), (400, 760).into());
        let rect = Placement::Rect(middle);
        assert_eq!(
            states_for(rect, middle, area),
            [S::TiledTop, S::TiledBottom]
        );
        let floating = Rectangle::new((100, 100).into(), (400, 300).into());
        assert!(states_for(Placement::Rect(floating), floating, area).is_empty());
    }

    #[test]
    fn growing_moves_one_edge_out_and_stops_at_the_work_area() {
        let area = Rectangle::new((0, 41).into(), (1000, 700).into());
        let rect = Rectangle::new((100, 100).into(), (400, 300).into());
        let none = Size::from((0, 0));
        assert_eq!(
            grown(rect, area, Snap::Right, 100, none),
            Rectangle::new((100, 100).into(), (500, 300).into())
        );
        assert_eq!(
            grown(rect, area, Snap::Left, 100, none),
            Rectangle::new((0, 100).into(), (500, 300).into())
        );
        assert_eq!(
            grown(rect, area, Snap::Up, 100, none),
            Rectangle::new((100, 41).into(), (400, 359).into()),
            "stops at the bar"
        );
        assert_eq!(
            grown(rect, area, Snap::Down, 100, none),
            Rectangle::new((100, 100).into(), (400, 400).into())
        );
    }

    #[test]
    fn growing_never_shrinks_and_keeps_to_the_apps_maximum() {
        let area = Rectangle::new((0, 0).into(), (1000, 700).into());
        // Already past the right edge: growing right leaves it.
        let wide = Rectangle::new((800, 0).into(), (400, 300).into());
        assert_eq!(
            grown(wide, area, Snap::Right, 100, Size::from((0, 0))),
            wide
        );
        // Grows left only up to the app's 450 px maximum, right edge still.
        let rect = Rectangle::new((300, 0).into(), (400, 300).into());
        assert_eq!(
            grown(rect, area, Snap::Left, 100, Size::from((450, 0))),
            Rectangle::new((250, 0).into(), (450, 300).into())
        );
    }

    #[test]
    fn resizing_keeps_the_centre_and_stays_inside() {
        let area = Rectangle::new((0, 0).into(), (1000, 700).into());
        let rect = Rectangle::new((300, 200).into(), (400, 300).into());
        let none = Size::from((0, 0));
        // Grow: 100 wider and taller, centre still at (500, 350).
        assert_eq!(
            resized_around_centre(rect, area, 100, 100, none, none),
            Rectangle::new((250, 150).into(), (500, 400).into())
        );
        // Narrower: only the width changes.
        assert_eq!(
            resized_around_centre(rect, area, -100, 0, none, none),
            Rectangle::new((350, 200).into(), (300, 300).into())
        );
        // Shrinking stops at 120 x 80, or the app's minimum if larger.
        let small = resized_around_centre(rect, area, -1000, -1000, none, none);
        assert_eq!(small.size, Size::from((120, 80)));
        let app_min = resized_around_centre(rect, area, -1000, 0, Size::from((350, 0)), none);
        assert_eq!(app_min.size.w, 350);
        // Growing near the edge moves it back inside, and never past the area.
        let corner = Rectangle::new((0, 0).into(), (400, 300).into());
        let grown_at_corner = resized_around_centre(corner, area, 100, 100, none, none);
        assert_eq!(grown_at_corner.loc, Point::from((0, 0)));
        let huge = resized_around_centre(rect, area, 5000, 5000, none, none);
        assert_eq!(huge, area);
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
