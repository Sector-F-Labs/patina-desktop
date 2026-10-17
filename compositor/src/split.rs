//! Dragging the line between tiled windows (ADR-0054): where windows the
//! tiler placed side by side meet, the gap between them (or, without gaps,
//! a few pixels either side of where they touch) shows a left-right arrow,
//! and dragging it grows the windows on one side as it shrinks those on the
//! other, so the gap stays as it was. Every window on either side of the
//! line moves with it. Afterwards the tiler hears each window's new
//! rectangle (`Resized`), so it keeps the split.

use smithay::{
    desktop::Window,
    input::pointer::{
        AxisFrame, ButtonEvent, CursorIcon, GestureHoldBeginEvent, GestureHoldEndEvent,
        GesturePinchBeginEvent, GesturePinchEndEvent, GesturePinchUpdateEvent,
        GestureSwipeBeginEvent, GestureSwipeEndEvent, GestureSwipeUpdateEvent, GrabStartData,
        MotionEvent, PointerGrab, PointerInnerHandle, RelativeMotionEvent,
    },
    reexports::{
        wayland_protocols::xdg::shell::server::xdg_toplevel,
        wayland_server::protocol::wl_surface::WlSurface,
    },
    utils::{Logical, Point, Rectangle},
    wayland::{compositor, shell::xdg::SurfaceCachedState},
};

use crate::grabs::forward_the_rest;
use crate::maximize::{Placement, placement};
use crate::state::State;

/// How far either side of a line where windows touch the pointer may be
/// to take it, in logical pixels; with gaps, the gap counts too.
const HANDLE: i32 = 4;

/// The narrowest a window gets by dragging the line, whatever the app
/// allows.
const NARROWEST: i32 = 120;

/// A line between tiled windows: the right edge of those on its left,
/// where those on its right begin, and which windows (by index) are on
/// each side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub left_edge: i32,
    pub right_edge: i32,
    pub left: Vec<usize>,
    pub right: Vec<usize>,
}

fn right_of(r: &Rectangle<i32, Logical>) -> i32 {
    r.loc.x + r.size.w
}

/// The line under `pointer` between `rects` (tiled windows), `gap` apart:
/// a pair side by side with the pointer between them, and every other
/// rectangle along the same edges.
pub fn line_under(
    rects: &[Rectangle<i32, Logical>],
    pointer: Point<f64, Logical>,
    gap: i32,
) -> Option<Line> {
    let (x, y) = (pointer.x, pointer.y);
    let spans =
        |r: &Rectangle<i32, Logical>| y >= r.loc.y as f64 && y < (r.loc.y + r.size.h) as f64;
    for a in rects.iter().filter(|r| spans(r)) {
        let left_edge = right_of(a);
        let right_edge = left_edge + gap;
        let near = x >= (left_edge - HANDLE) as f64 && x < (right_edge + HANDLE) as f64;
        if !near || !rects.iter().any(|b| b.loc.x == right_edge && spans(b)) {
            continue;
        }
        let side = |edge: fn(&Rectangle<i32, Logical>) -> i32, at: i32| {
            rects
                .iter()
                .enumerate()
                .filter(|(_, r)| edge(r) == at)
                .map(|(i, _)| i)
                .collect()
        };
        return Some(Line {
            left_edge,
            right_edge,
            left: side(right_of, left_edge),
            right: side(|r| r.loc.x, right_edge),
        });
    }
    None
}

/// How far the line may move by `dx`: no window on either side narrower
/// than its `min` width (or `NARROWEST`).
pub fn allowed(dx: i32, left: &[(i32, i32)], right: &[(i32, i32)]) -> i32 {
    let narrowest = |min: i32| min.max(NARROWEST);
    // The left windows shrink as the line goes left; the right ones as it
    // goes right.
    let lowest = left
        .iter()
        .map(|&(w, min)| (narrowest(min) - w).min(0))
        .max()
        .unwrap_or(i32::MIN);
    let highest = right
        .iter()
        .map(|&(w, min)| (w - narrowest(min)).max(0))
        .min()
        .unwrap_or(i32::MAX);
    dx.clamp(lowest, highest)
}

/// A tiled window and where it is.
type Tiled = (Window, Rectangle<i32, Logical>);

/// Dragging a line: the windows on each side and where they began.
pub struct SplitGrab {
    start: GrabStartData<State>,
    left: Vec<Tiled>,
    right: Vec<Tiled>,
    /// How far the line has moved.
    moved: i32,
}

/// `window`'s narrowest width, as the app asks.
fn min_width(window: &Window) -> i32 {
    window.toplevel().map_or(0, |t| {
        compositor::with_states(t.wl_surface(), |states| {
            states
                .cached_state
                .get::<SurfaceCachedState>()
                .current()
                .min_size
                .w
        })
    })
}

impl SplitGrab {
    fn rects(&self, dx: i32) -> Vec<Tiled> {
        let left = self.left.iter().map(|(w, r)| {
            let r = Rectangle::new(r.loc, (r.size.w + dx, r.size.h).into());
            (w.clone(), r)
        });
        let right = self.right.iter().map(|(w, r)| {
            let r = Rectangle::new(
                (r.loc.x + dx, r.loc.y).into(),
                (r.size.w - dx, r.size.h).into(),
            );
            (w.clone(), r)
        });
        left.chain(right).collect()
    }
}

impl PointerGrab<State> for SplitGrab {
    fn motion(
        &mut self,
        state: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        _: Option<(WlSurface, Point<f64, Logical>)>,
        event: &MotionEvent,
    ) {
        handle.motion(state, None, event);
        let widths = |side: &[Tiled]| -> Vec<(i32, i32)> {
            side.iter().map(|(w, r)| (r.size.w, min_width(w))).collect()
        };
        let wanted = (event.location.x - self.start.location.x).round() as i32;
        let dx = allowed(wanted, &widths(&self.left), &widths(&self.right));
        if dx == self.moved {
            return;
        }
        self.moved = dx;
        for (window, rect) in self.rects(dx) {
            state.retile(&window, rect, true);
        }
    }

    fn button(
        &mut self,
        state: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        event: &ButtonEvent,
    ) {
        handle.button(state, event);
        if !handle.current_pressed().is_empty() {
            return;
        }
        handle.unset_grab(self, state, event.serial, event.time, true);
        state.end_grab_cursor();
        for (window, rect) in self.rects(self.moved) {
            state.retile(&window, rect, false);
            state.window_resized(&window, rect);
        }
    }

    forward_the_rest!();
}

impl State {
    /// The windows the tiler placed (not maximised or filling, which have
    /// no neighbours), with where they are.
    fn tiled_windows(&self) -> Vec<Tiled> {
        self.space
            .elements()
            .filter_map(|w| match placement(w) {
                Some(Placement::Rect(rect)) if w.toplevel().is_some() => Some((w.clone(), rect)),
                _ => None,
            })
            .collect()
    }

    /// The line between tiled windows under `pointer`, if it's showing
    /// there: not under the bar, a menu, a fullscreen window or a window
    /// that isn't one of the line's.
    fn split_under(&self, pointer: Point<f64, Logical>) -> Option<(Line, Vec<Tiled>)> {
        if self.locked() || self.fullscreen.is_some() {
            return None;
        }
        let map = smithay::desktop::layer_map_for_output(&self.output);
        use smithay::wayland::shell::wlr_layer::Layer;
        if map.layer_under(Layer::Overlay, pointer).is_some()
            || map.layer_under(Layer::Top, pointer).is_some()
        {
            return None;
        }
        let tiled = self.tiled_windows();
        let rects: Vec<_> = tiled.iter().map(|(_, r)| *r).collect();
        let line = line_under(&rects, pointer, self.config.gap())?;
        let on_line = |w: &Window| {
            line.left
                .iter()
                .chain(&line.right)
                .any(|&i| &tiled[i].0 == w)
        };
        if let Some((top, _)) = self.space.element_under(pointer)
            && !on_line(top)
        {
            return None;
        }
        Some((line, tiled))
    }

    /// Whether the pointer at `pointer` is on a line between tiled
    /// windows: it shows the arrow there, and windows don't get it.
    pub fn hover_split(&mut self, pointer: Point<f64, Logical>, grabbed: bool) -> bool {
        if grabbed {
            return false;
        }
        let over = self.split_under(pointer).is_some();
        if over {
            self.grab_cursor = Some(CursorIcon::EwResize);
        } else if self.split_hover {
            self.grab_cursor = None;
        }
        if over != self.split_hover {
            self.split_hover = over;
            self.needs_redraw = true;
        }
        over
    }

    /// Starts dragging the line under the pointer, if there is one.
    pub fn start_split(&mut self, start: GrabStartData<State>) -> Option<SplitGrab> {
        let (line, tiled) = self.split_under(start.location)?;
        let side =
            |indices: &[usize]| -> Vec<_> { indices.iter().map(|&i| tiled[i].clone()).collect() };
        let grab = SplitGrab {
            start,
            left: side(&line.left),
            right: side(&line.right),
            moved: 0,
        };
        for (window, rect) in grab.rects(0) {
            self.retile(&window, rect, true);
        }
        self.grab_cursor = Some(CursorIcon::EwResize);
        Some(grab)
    }
}

/// While the line moves, the windows resize as in a pointer resize.
pub fn set_resizing(window: &Window, on: bool) {
    if let Some(toplevel) = window.toplevel() {
        toplevel.with_pending_state(|s| {
            if on {
                s.states.set(xdg_toplevel::State::Resizing);
            } else {
                s.states.unset(xdg_toplevel::State::Resizing);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: i32, y: i32, w: i32, h: i32) -> Rectangle<i32, Logical> {
        Rectangle::new((x, y).into(), (w, h).into())
    }

    #[test]
    fn the_gap_between_halves_is_a_line() {
        let halves = [rect(8, 8, 629, 743), rect(645, 8, 628, 743)];
        let line = line_under(&halves, (640.0, 300.0).into(), 8).unwrap();
        assert_eq!((line.left_edge, line.right_edge), (637, 645));
        assert_eq!((line.left, line.right), (vec![0], vec![1]));
        // A little past either side counts; further doesn't.
        assert!(line_under(&halves, (634.0, 300.0).into(), 8).is_some());
        assert!(line_under(&halves, (648.0, 300.0).into(), 8).is_some());
        assert!(line_under(&halves, (620.0, 300.0).into(), 8).is_none());
        // Not above or below the windows.
        assert!(line_under(&halves, (640.0, 900.0).into(), 8).is_none());
    }

    #[test]
    fn without_gaps_the_line_is_where_they_touch() {
        let halves = [rect(0, 0, 640, 760), rect(640, 0, 640, 760)];
        assert!(line_under(&halves, (637.0, 10.0).into(), 0).is_some());
        assert!(line_under(&halves, (642.0, 10.0).into(), 0).is_some());
        assert!(line_under(&halves, (600.0, 10.0).into(), 0).is_none());
    }

    #[test]
    fn windows_apart_by_more_than_the_gap_have_no_line() {
        let apart = [rect(0, 0, 600, 760), rect(640, 0, 640, 760)];
        assert!(line_under(&apart, (620.0, 10.0).into(), 8).is_none());
    }

    #[test]
    fn every_window_along_the_line_moves_with_it() {
        // Two stacked on the left, one on the right.
        let tiled = [
            rect(0, 0, 640, 380),
            rect(0, 380, 640, 380),
            rect(640, 0, 640, 760),
        ];
        let line = line_under(&tiled, (640.0, 100.0).into(), 0).unwrap();
        assert_eq!((line.left, line.right), (vec![0, 1], vec![2]));
    }

    #[test]
    fn the_line_stops_before_a_window_gets_too_narrow() {
        let left = [(600, 0)];
        let right = [(600, 300)];
        assert_eq!(allowed(-100, &left, &right), -100);
        assert_eq!(allowed(-1000, &left, &right), -(600 - NARROWEST));
        assert_eq!(allowed(1000, &left, &right), 300, "the app's own minimum");
        assert_eq!(allowed(0, &left, &right), 0);
    }
}
