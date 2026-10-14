//! Moving and resizing windows with the pointer: started by a client's own
//! frame (`xdg_toplevel.move` / `resize`, as Patina's header bar sends), or
//! by Super+drag on any window. Adapted from Smithay's smallvil.

use std::cell::RefCell;

use smithay::{
    desktop::Window,
    input::pointer::{
        AxisFrame, ButtonEvent, GestureHoldBeginEvent, GestureHoldEndEvent, GesturePinchBeginEvent,
        GesturePinchEndEvent, GesturePinchUpdateEvent, GestureSwipeBeginEvent,
        GestureSwipeEndEvent, GestureSwipeUpdateEvent, GrabStartData, MotionEvent, PointerGrab,
        PointerInnerHandle, RelativeMotionEvent,
    },
    reexports::{
        wayland_protocols::xdg::shell::server::xdg_toplevel,
        wayland_server::protocol::wl_surface::WlSurface,
    },
    utils::{Logical, Point, Rectangle, Size},
    wayland::{compositor, shell::xdg::SurfaceCachedState},
};

use crate::state::State;

/// The parts of `PointerGrab` both grabs pass straight through.
macro_rules! forward_the_rest {
    () => {
        fn relative_motion(
            &mut self,
            state: &mut State,
            handle: &mut PointerInnerHandle<'_, State>,
            focus: Option<(WlSurface, Point<f64, Logical>)>,
            event: &RelativeMotionEvent,
        ) {
            handle.relative_motion(state, focus, event);
        }
        fn axis(
            &mut self,
            state: &mut State,
            handle: &mut PointerInnerHandle<'_, State>,
            details: AxisFrame,
        ) {
            handle.axis(state, details)
        }
        fn frame(&mut self, state: &mut State, handle: &mut PointerInnerHandle<'_, State>) {
            handle.frame(state);
        }
        fn gesture_swipe_begin(
            &mut self,
            s: &mut State,
            h: &mut PointerInnerHandle<'_, State>,
            e: &GestureSwipeBeginEvent,
        ) {
            h.gesture_swipe_begin(s, e)
        }
        fn gesture_swipe_update(
            &mut self,
            s: &mut State,
            h: &mut PointerInnerHandle<'_, State>,
            e: &GestureSwipeUpdateEvent,
        ) {
            h.gesture_swipe_update(s, e)
        }
        fn gesture_swipe_end(
            &mut self,
            s: &mut State,
            h: &mut PointerInnerHandle<'_, State>,
            e: &GestureSwipeEndEvent,
        ) {
            h.gesture_swipe_end(s, e)
        }
        fn gesture_pinch_begin(
            &mut self,
            s: &mut State,
            h: &mut PointerInnerHandle<'_, State>,
            e: &GesturePinchBeginEvent,
        ) {
            h.gesture_pinch_begin(s, e)
        }
        fn gesture_pinch_update(
            &mut self,
            s: &mut State,
            h: &mut PointerInnerHandle<'_, State>,
            e: &GesturePinchUpdateEvent,
        ) {
            h.gesture_pinch_update(s, e)
        }
        fn gesture_pinch_end(
            &mut self,
            s: &mut State,
            h: &mut PointerInnerHandle<'_, State>,
            e: &GesturePinchEndEvent,
        ) {
            h.gesture_pinch_end(s, e)
        }
        fn gesture_hold_begin(
            &mut self,
            s: &mut State,
            h: &mut PointerInnerHandle<'_, State>,
            e: &GestureHoldBeginEvent,
        ) {
            h.gesture_hold_begin(s, e)
        }
        fn gesture_hold_end(
            &mut self,
            s: &mut State,
            h: &mut PointerInnerHandle<'_, State>,
            e: &GestureHoldEndEvent,
        ) {
            h.gesture_hold_end(s, e)
        }
        fn start_data(&self) -> &GrabStartData<State> {
            &self.start
        }
        fn unset(&mut self, _: &mut State) {}
    };
}

/// linux/input-event-codes.h
pub const BTN_LEFT: u32 = 0x110;

/// The grab ends when no button is held any more.
fn released(handle: &PointerInnerHandle<'_, State>) -> bool {
    handle.current_pressed().is_empty()
}

pub struct MoveGrab {
    pub start: GrabStartData<State>,
    pub window: Window,
    pub initial: Point<i32, Logical>,
}

impl PointerGrab<State> for MoveGrab {
    fn motion(
        &mut self,
        state: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        _: Option<(WlSurface, Point<f64, Logical>)>,
        event: &MotionEvent,
    ) {
        // No client has the pointer while it drags a window.
        handle.motion(state, None, event);
        let location = self.initial.to_f64() + (event.location - self.start.location);
        state.place(&self.window, location.to_i32_round());
        state.needs_redraw = true;
    }

    fn button(
        &mut self,
        state: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        event: &ButtonEvent,
    ) {
        handle.button(state, event);
        if released(handle) {
            handle.unset_grab(self, state, event.serial, event.time, true);
        }
    }

    forward_the_rest!();
}

/// Which edges a resize moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Edges {
    pub top: bool,
    pub bottom: bool,
    pub left: bool,
    pub right: bool,
}

impl From<xdg_toplevel::ResizeEdge> for Edges {
    fn from(edge: xdg_toplevel::ResizeEdge) -> Self {
        use xdg_toplevel::ResizeEdge as E;
        Self {
            top: matches!(edge, E::Top | E::TopLeft | E::TopRight),
            bottom: matches!(edge, E::Bottom | E::BottomLeft | E::BottomRight),
            left: matches!(edge, E::Left | E::TopLeft | E::BottomLeft),
            right: matches!(edge, E::Right | E::TopRight | E::BottomRight),
        }
    }
}

pub struct ResizeGrab {
    start: GrabStartData<State>,
    window: Window,
    edges: Edges,
    initial: Rectangle<i32, Logical>,
    last_size: Size<i32, Logical>,
}

impl ResizeGrab {
    pub fn start(
        start: GrabStartData<State>,
        window: Window,
        edges: Edges,
        initial: Rectangle<i32, Logical>,
    ) -> Option<Self> {
        let toplevel = window.toplevel()?;
        ResizeState::with(toplevel.wl_surface(), |s| {
            *s = ResizeState::Resizing { edges, initial }
        });
        toplevel.with_pending_state(|s| {
            s.states.set(xdg_toplevel::State::Resizing);
        });
        toplevel.send_pending_configure();
        Some(Self {
            start,
            window,
            edges,
            initial,
            last_size: initial.size,
        })
    }
}

/// The size a resize from `initial` gives after the pointer moved by
/// `delta`, within the client's limits (0 means none).
pub fn resized(
    initial: Size<i32, Logical>,
    edges: Edges,
    delta: Point<f64, Logical>,
    min: Size<i32, Logical>,
    max: Size<i32, Logical>,
) -> Size<i32, Logical> {
    let mut w = initial.w;
    let mut h = initial.h;
    if edges.left {
        w = (initial.w as f64 - delta.x) as i32;
    } else if edges.right {
        w = (initial.w as f64 + delta.x) as i32;
    }
    if edges.top {
        h = (initial.h as f64 - delta.y) as i32;
    } else if edges.bottom {
        h = (initial.h as f64 + delta.y) as i32;
    }
    let limit =
        |v: i32, min: i32, max: i32| v.max(min.max(1)).min(if max == 0 { i32::MAX } else { max });
    Size::from((limit(w, min.w, max.w), limit(h, min.h, max.h)))
}

impl PointerGrab<State> for ResizeGrab {
    fn motion(
        &mut self,
        state: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        _: Option<(WlSurface, Point<f64, Logical>)>,
        event: &MotionEvent,
    ) {
        handle.motion(state, None, event);
        let Some(toplevel) = self.window.toplevel() else {
            return;
        };
        let (min, max) = compositor::with_states(toplevel.wl_surface(), |states| {
            let mut cached = states.cached_state.get::<SurfaceCachedState>();
            let current = cached.current();
            (current.min_size, current.max_size)
        });
        self.last_size = resized(
            self.initial.size,
            self.edges,
            event.location - self.start.location,
            min,
            max,
        );
        toplevel.with_pending_state(|s| {
            s.states.set(xdg_toplevel::State::Resizing);
            s.size = Some(self.last_size);
        });
        toplevel.send_pending_configure();
    }

    fn button(
        &mut self,
        state: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        event: &ButtonEvent,
    ) {
        handle.button(state, event);
        if !released(handle) {
            return;
        }
        handle.unset_grab(self, state, event.serial, event.time, true);
        let Some(toplevel) = self.window.toplevel() else {
            return;
        };
        toplevel.with_pending_state(|s| {
            s.states.unset(xdg_toplevel::State::Resizing);
            s.size = Some(self.last_size);
        });
        toplevel.send_pending_configure();
        let (edges, initial) = (self.edges, self.initial);
        ResizeState::with(toplevel.wl_surface(), |s| {
            *s = ResizeState::WaitingForLastCommit { edges, initial }
        });
    }

    forward_the_rest!();
}

/// A resize in progress, kept on the surface so commits can move the
/// window as its top or left edge moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum ResizeState {
    #[default]
    Idle,
    Resizing {
        edges: Edges,
        initial: Rectangle<i32, Logical>,
    },
    /// The pointer let go; the next commit has the final size.
    WaitingForLastCommit {
        edges: Edges,
        initial: Rectangle<i32, Logical>,
    },
}

impl ResizeState {
    fn with<T>(surface: &WlSurface, f: impl FnOnce(&mut Self) -> T) -> T {
        compositor::with_states(surface, |states| {
            states.data_map.insert_if_missing(RefCell::<Self>::default);
            let cell = states
                .data_map
                .get::<RefCell<Self>>()
                .expect("inserted above");
            f(&mut cell.borrow_mut())
        })
    }
}

/// On a commit from a window being resized by its top or left edge, moves
/// the window so the opposite edge stays put.
pub fn handle_commit(state: &mut State, surface: &WlSurface) {
    let Some(window) = state
        .space
        .elements()
        .find(|w| w.toplevel().is_some_and(|t| t.wl_surface() == surface))
        .cloned()
    else {
        return;
    };
    let Some(mut location) = state.space.element_location(&window) else {
        return;
    };
    let size = window.geometry().size;
    let resize = ResizeState::with(surface, |s| match *s {
        ResizeState::Resizing { edges, initial } => Some((edges, initial)),
        ResizeState::WaitingForLastCommit { edges, initial } => {
            *s = ResizeState::Idle;
            Some((edges, initial))
        }
        ResizeState::Idle => None,
    });
    let Some((edges, initial)) = resize else {
        return;
    };
    if edges.left {
        location.x = initial.loc.x + initial.size.w - size.w;
    }
    if edges.top {
        location.y = initial.loc.y + initial.size.h - size.h;
    }
    if edges.left || edges.top {
        state.place(&window, location);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn size(w: i32, h: i32) -> Size<i32, Logical> {
        Size::from((w, h))
    }

    #[test]
    fn right_and_bottom_grow_with_the_pointer() {
        let e = Edges {
            bottom: true,
            right: true,
            ..Default::default()
        };
        assert_eq!(
            resized(
                size(400, 300),
                e,
                (50.0, 20.0).into(),
                size(0, 0),
                size(0, 0)
            ),
            size(450, 320)
        );
    }

    #[test]
    fn left_and_top_grow_against_the_pointer() {
        let e = Edges {
            top: true,
            left: true,
            ..Default::default()
        };
        assert_eq!(
            resized(
                size(400, 300),
                e,
                (-50.0, -20.0).into(),
                size(0, 0),
                size(0, 0)
            ),
            size(450, 320)
        );
    }

    #[test]
    fn one_edge_leaves_the_other_axis_alone() {
        let e = Edges {
            right: true,
            ..Default::default()
        };
        assert_eq!(
            resized(
                size(400, 300),
                e,
                (10.0, 99.0).into(),
                size(0, 0),
                size(0, 0)
            ),
            size(410, 300)
        );
    }

    #[test]
    fn keeps_to_the_clients_limits() {
        let e = Edges {
            right: true,
            bottom: true,
            ..Default::default()
        };
        assert_eq!(
            resized(
                size(400, 300),
                e,
                (-1000.0, 1000.0).into(),
                size(200, 0),
                size(0, 500)
            ),
            size(200, 500)
        );
        assert_eq!(
            resized(
                size(10, 10),
                e,
                (-1000.0, -1000.0).into(),
                size(0, 0),
                size(0, 0)
            ),
            size(1, 1)
        );
    }

    #[test]
    fn edges_from_xdg() {
        let e: Edges = xdg_toplevel::ResizeEdge::TopLeft.into();
        assert_eq!(
            e,
            Edges {
                top: true,
                left: true,
                ..Default::default()
            }
        );
        assert_eq!(
            Edges::from(xdg_toplevel::ResizeEdge::None),
            Edges::default()
        );
    }
}
