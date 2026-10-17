//! X11 apps through Xwayland (ADR-0040): Steam, Discord, Wine and the rest
//! that only speak X. Xwayland is an X server that is itself a Wayland
//! client; the compositor starts it, and is the window manager for its
//! windows (Smithay's `X11Wm` calls the `XwmHandler` here). X windows live in
//! the same space as Wayland ones, as Smithay `Window`s, so they're drawn,
//! focused, tabbed and moved like any other.

use std::process::Stdio;
use std::sync::OnceLock;

use smithay::{
    delegate_xwayland_shell,
    desktop::Window,
    input::pointer::{
        AxisFrame, ButtonEvent, Focus, GestureHoldBeginEvent, GestureHoldEndEvent,
        GesturePinchBeginEvent, GesturePinchEndEvent, GesturePinchUpdateEvent,
        GestureSwipeBeginEvent, GestureSwipeEndEvent, GestureSwipeUpdateEvent, GrabStartData,
        MotionEvent, PointerGrab, PointerInnerHandle, RelativeMotionEvent,
    },
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Logical, Point, Rectangle, SERIAL_COUNTER, Size},
    wayland::xwayland_shell::{XWaylandShellHandler, XWaylandShellState},
    xwayland::{
        X11Surface, X11Wm, XWayland, XWaylandEvent, XwmHandler,
        xwm::{Reorder, ResizeEdge, XwmId},
    },
};

use crate::grabs::Edges;
use crate::state::State;

/// The X display clients get as `DISPLAY` (":1"), once Xwayland is spawned.
static DISPLAY: OnceLock<String> = OnceLock::new();

/// `DISPLAY` for clients, if Xwayland is running.
pub fn display() -> Option<&'static str> {
    DISPLAY.get().map(String::as_str)
}

/// The smallest a resize makes an X window.
const MIN_SIZE: i32 = 64;

impl State {
    /// Starts Xwayland on a free display, and the window manager once it's
    /// ready. Without Xwayland installed, X apps just don't open.
    pub fn start_xwayland(&mut self) {
        let spawned = XWayland::spawn(
            &self.display_handle,
            None,
            std::iter::empty::<(String, String)>(),
            true,
            Stdio::null(),
            Stdio::null(),
            |_| (),
        );
        let (xwayland, client) = match spawned {
            Ok(x) => x,
            Err(e) => {
                eprintln!("patina-compositor: no Xwayland, so no X11 apps: {e}");
                return;
            }
        };
        let display = format!(":{}", xwayland.display_number());
        let inserted =
            self.loop_handle
                .insert_source(xwayland, move |event, _, state| match event {
                    XWaylandEvent::Ready { x11_socket, .. } => {
                        match X11Wm::start_wm(state.loop_handle.clone(), x11_socket, client.clone())
                        {
                            Ok(wm) => state.xwm = Some(wm),
                            Err(e) => eprintln!("patina-compositor: X11 window manager: {e}"),
                        }
                    }
                    XWaylandEvent::Error => {
                        eprintln!("patina-compositor: Xwayland stopped while starting");
                    }
                });
        match inserted {
            Ok(_) => {
                eprintln!("patina-compositor: X11 apps connect on DISPLAY={display}");
                let _ = DISPLAY.set(display);
            }
            Err(e) => eprintln!("patina-compositor: Xwayland: {e}"),
        }
    }

    /// The window showing `surface`, if it's mapped.
    fn x11_window(&self, surface: &X11Surface) -> Option<Window> {
        self.space
            .elements()
            .find(|w| w.x11_surface() == Some(surface))
            .cloned()
    }

    /// Tells the X server where `window` is, if it's an X window, so its
    /// menus and tooltips open beside it.
    pub fn tell_x11_where(&mut self, window: &Window, location: Point<i32, Logical>) {
        if let Some(x11) = window.x11_surface()
            && !x11.is_override_redirect()
        {
            let _ = x11.configure(Rectangle::new(location, x11.geometry().size));
        }
    }

    /// Raises `window` among X windows too, so X's idea of what's on top
    /// matches what's shown.
    pub fn raise_x11(&mut self, window: &Window) {
        if let (Some(xwm), Some(x11)) = (self.xwm.as_mut(), window.x11_surface()) {
            let _ = xwm.raise_window(x11);
        }
    }

    /// The pointer grab a move or resize asked for by an X window continues,
    /// if the pointer is held on that window.
    fn x11_grab(&self, window: &X11Surface) -> Option<(GrabStartData<State>, Window)> {
        let pointer = self.seat.get_pointer()?;
        if !pointer.is_grabbed() {
            return None;
        }
        let start = pointer.grab_start_data()?;
        let surface = window.wl_surface()?;
        let (focus, _) = start.focus.as_ref()?;
        if !smithay::reexports::wayland_server::Resource::id(focus)
            .same_client_as(&smithay::reexports::wayland_server::Resource::id(&surface))
        {
            return None;
        }
        Some((start, self.x11_window(window)?))
    }
}

impl XwmHandler for State {
    fn xwm_state(&mut self, _: XwmId) -> &mut X11Wm {
        self.xwm
            .as_mut()
            .expect("the X11 window manager is running")
    }

    fn new_window(&mut self, _: XwmId, _: X11Surface) {}

    fn new_override_redirect_window(&mut self, _: XwmId, _: X11Surface) {}

    fn map_window_request(&mut self, _: XwmId, surface: X11Surface) {
        if surface.set_mapped(true).is_err() {
            return;
        }
        let window = Window::new_x11_window(surface.clone());
        let geometry = surface.geometry();
        // A game that asked for fullscreen before showing, or sized itself
        // to the screen (ADR-0041), goes straight there: Wine takes a
        // configure anywhere else first as the window moving.
        if surface.is_fullscreen() || self.is_screen(geometry) {
            self.space
                .map_element(window.clone(), self.screen().loc, false);
            self.fullscreen(&window);
            return;
        }
        // Where it asked to be, as Steam's dialogs do; otherwise cascaded.
        let location = if geometry.loc != Point::from((0, 0)) {
            geometry.loc
        } else {
            self.cascade()
        };
        self.space.map_element(window.clone(), location, false);
        let _ = surface.configure(Rectangle::new(location, geometry.size));
        self.focus_window(window.clone());
    }

    fn mapped_override_redirect_window(&mut self, _: XwmId, surface: X11Surface) {
        // Menus and tooltips: where they put themselves, never focused.
        let location = surface.geometry().loc;
        let window = Window::new_x11_window(surface);
        self.space.map_element(window, location, false);
        self.needs_redraw = true;
    }

    fn unmapped_window(&mut self, _: XwmId, surface: X11Surface) {
        self.forget_x11(&surface);
        if !surface.is_override_redirect() {
            let _ = surface.set_mapped(false);
        }
    }

    fn destroyed_window(&mut self, _: XwmId, surface: X11Surface) {
        self.forget_x11(&surface);
    }

    fn configure_request(
        &mut self,
        _: XwmId,
        surface: X11Surface,
        x: Option<i32>,
        y: Option<i32>,
        w: Option<u32>,
        h: Option<u32>,
        _: Option<Reorder>,
    ) {
        let mut geometry = surface.geometry();
        if let Some(w) = w {
            geometry.size.w = w as i32;
        }
        if let Some(h) = h {
            geometry.size.h = h as i32;
        }
        let requested = Rectangle::new(
            (x.unwrap_or(geometry.loc.x), y.unwrap_or(geometry.loc.y)).into(),
            geometry.size,
        );
        // A game asking for the whole screen means fullscreen (ADR-0041).
        if self.is_screen(requested)
            && let Some(window) = self.x11_window(&surface)
        {
            self.fullscreen(&window);
            return;
        }
        // A window on screen stays where the compositor put it; one not
        // shown yet may say where it wants to be.
        match self
            .x11_window(&surface)
            .and_then(|w| self.space.element_location(&w))
        {
            Some(location) => geometry.loc = location,
            None => {
                if let Some(x) = x {
                    geometry.loc.x = x;
                }
                if let Some(y) = y {
                    geometry.loc.y = y;
                }
            }
        }
        let _ = surface.configure(geometry);
    }

    fn configure_notify(
        &mut self,
        _: XwmId,
        surface: X11Surface,
        geometry: Rectangle<i32, Logical>,
        _: Option<smithay::xwayland::xwm::X11Window>,
    ) {
        // Menus and tooltips move themselves.
        if surface.is_override_redirect()
            && let Some(window) = self.x11_window(&surface)
        {
            self.space.map_element(window, geometry.loc, false);
            self.needs_redraw = true;
        }
    }

    fn fullscreen_request(&mut self, _: XwmId, surface: X11Surface) {
        match self.x11_window(&surface) {
            Some(window) => self.fullscreen(&window),
            // Not shown yet: it goes fullscreen when it maps.
            None => {
                let _ = surface.set_fullscreen(true);
            }
        }
    }

    fn unfullscreen_request(&mut self, _: XwmId, surface: X11Surface) {
        match self.x11_window(&surface) {
            Some(window) => self.unfullscreen(&window),
            None => {
                let _ = surface.set_fullscreen(false);
            }
        }
    }

    /// A window asking to be minimised, as a Windows game under Wine does
    /// when it loses focus in fullscreen. There's nowhere to minimise to, so
    /// it stays where it is, but it's told it's iconic: Wine restores the
    /// game when that ends, which focusing it again does (`restore_x11`).
    fn minimize_request(&mut self, _: XwmId, surface: X11Surface) {
        eprintln!(
            "patina-compositor: {:?} asks to be minimised",
            surface.title()
        );
        if let Err(e) = surface.set_suspended(true) {
            eprintln!("patina-compositor: can't minimise {:?}: {e}", surface.title());
        }
    }

    fn unminimize_request(&mut self, _: XwmId, surface: X11Surface) {
        eprintln!(
            "patina-compositor: {:?} asks to be restored",
            surface.title()
        );
        let _ = surface.set_suspended(false);
    }

    fn move_request(&mut self, _: XwmId, surface: X11Surface, _: u32) {
        let Some((start, window)) = self.x11_grab(&surface) else {
            return;
        };
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        let serial = SERIAL_COUNTER.next_serial();
        self.start_move(window, start, pointer, serial);
    }

    fn resize_request(&mut self, _: XwmId, surface: X11Surface, _: u32, edge: ResizeEdge) {
        let Some((start, window)) = self.x11_grab(&surface) else {
            return;
        };
        let Some(location) = self.space.element_location(&window) else {
            return;
        };
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        let edges = edges(edge);
        let grab = X11ResizeGrab {
            start,
            window,
            surface: surface.clone(),
            edges,
            initial: Rectangle::new(location, surface.geometry().size),
        };
        self.grab_cursor = Some(edges.cursor());
        pointer.set_grab(self, grab, SERIAL_COUNTER.next_serial(), Focus::Clear);
    }
}

impl State {
    /// An X window being focused that was minimised (`minimize_request`)
    /// is restored, so Wine knows its game is back and carries on.
    pub fn restore_x11(&self, window: &Window) {
        if let Some(x) = window.x11_surface()
            && x.is_minimized()
        {
            eprintln!("patina-compositor: restoring {:?}", x.title());
            let _ = x.set_suspended(false);
        }
    }

    /// Takes an X window off the screen, and gives the keyboard on if it
    /// had it.
    fn forget_x11(&mut self, surface: &X11Surface) {
        let Some(window) = self.x11_window(surface) else {
            return;
        };
        if self.fullscreen.as_ref() == Some(&window) {
            self.fullscreen = None;
        }
        self.space.unmap_elem(&window);
        self.needs_redraw = true;
        if surface.is_override_redirect() {
            return;
        }
        match surface.wl_surface() {
            Some(wl) => self.focus_after_close(&wl),
            None => self.focus_top_window(),
        }
    }
}

fn edges(edge: ResizeEdge) -> Edges {
    use ResizeEdge as E;
    Edges {
        top: matches!(edge, E::Top | E::TopLeft | E::TopRight),
        bottom: matches!(edge, E::Bottom | E::BottomLeft | E::BottomRight),
        left: matches!(edge, E::Left | E::TopLeft | E::BottomLeft),
        right: matches!(edge, E::Right | E::TopRight | E::BottomRight),
    }
}

/// The rectangle a resize from `edges` by `delta` makes of `initial`, no
/// smaller than `min`; the opposite edges stay put.
fn resized(
    initial: Rectangle<i32, Logical>,
    edges: Edges,
    delta: Point<i32, Logical>,
    min: Size<i32, Logical>,
) -> Rectangle<i32, Logical> {
    let mut rect = initial;
    if edges.right {
        rect.size.w = (initial.size.w + delta.x).max(min.w);
    }
    if edges.bottom {
        rect.size.h = (initial.size.h + delta.y).max(min.h);
    }
    if edges.left {
        rect.size.w = (initial.size.w - delta.x).max(min.w);
        rect.loc.x = initial.loc.x + initial.size.w - rect.size.w;
    }
    if edges.top {
        rect.size.h = (initial.size.h - delta.y).max(min.h);
        rect.loc.y = initial.loc.y + initial.size.h - rect.size.h;
    }
    rect
}

/// Resizing an X window: unlike an xdg toplevel, it takes the size it's
/// given, so each motion configures it and moves it if the left or top
/// edge moves.
struct X11ResizeGrab {
    start: GrabStartData<State>,
    window: Window,
    surface: X11Surface,
    edges: Edges,
    initial: Rectangle<i32, Logical>,
}

impl PointerGrab<State> for X11ResizeGrab {
    fn motion(
        &mut self,
        state: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        _: Option<(WlSurface, Point<f64, Logical>)>,
        event: &MotionEvent,
    ) {
        handle.motion(state, None, event);
        let delta = (event.location - self.start.location).to_i32_round();
        let min = self.surface.min_size().unwrap_or_default();
        let min = Size::from((min.w.max(MIN_SIZE), min.h.max(MIN_SIZE)));
        let rect = resized(self.initial, self.edges, delta, min);
        let _ = self.surface.configure(rect);
        state.place(&self.window, rect.loc);
    }

    fn button(
        &mut self,
        state: &mut State,
        handle: &mut PointerInnerHandle<'_, State>,
        event: &ButtonEvent,
    ) {
        handle.button(state, event);
        if handle.current_pressed().is_empty() {
            handle.unset_grab(self, state, event.serial, event.time, true);
            state.end_grab_cursor();
        }
    }

    crate::grabs::forward_the_rest!();
}

impl XWaylandShellHandler for State {
    fn xwayland_shell_state(&mut self) -> &mut XWaylandShellState {
        &mut self.xwayland_shell
    }
}

delegate_xwayland_shell!(State);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resizing_from_the_left_keeps_the_right_edge() {
        let initial = Rectangle::new((100, 100).into(), (400, 300).into());
        let min = Size::from((64, 64));
        let left = edges(ResizeEdge::Left);
        let r = resized(initial, left, (50, 0).into(), min);
        assert_eq!((r.loc.x, r.size.w), (150, 350));
        assert_eq!(r.loc.x + r.size.w, 500);
        // Never smaller than the minimum, still anchored on the right.
        let r = resized(initial, left, (1000, 0).into(), min);
        assert_eq!((r.loc.x, r.size.w), (436, 64));
        let r = resized(
            initial,
            edges(ResizeEdge::BottomRight),
            (20, -40).into(),
            min,
        );
        assert_eq!(r, Rectangle::new((100, 100).into(), (420, 260).into()));
    }
}
