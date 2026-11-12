//! Fullscreen (ADR-0041): a game or video asking for the whole screen gets
//! it, over the bar. Xwayland games ask with `_NET_WM_STATE_FULLSCREEN`, or
//! by asking for the screen's size at its corner; Wayland apps with
//! `xdg_toplevel.set_fullscreen`. The window is placed over the whole
//! output and drawn above the layer surfaces while it has the keyboard, but
//! for the Overlay layer, which layer-shell puts over it (a notice, dictation's
//! pill); leaving fullscreen puts it back where it was.

use smithay::{
    backend::renderer::{
        ImportAll, Renderer, Texture,
        element::{AsRenderElements, surface::WaylandSurfaceRenderElement},
    },
    desktop::{Window, WindowSurfaceType},
    reexports::{
        wayland_protocols::xdg::shell::server::xdg_toplevel,
        wayland_server::protocol::wl_surface::WlSurface,
    },
    utils::{Logical, Point, Rectangle, Scale},
};

use crate::state::State;

/// Where a fullscreen window was before, kept on the window.
#[derive(Default)]
struct Before(std::cell::Cell<Option<Rectangle<i32, Logical>>>);

fn before(window: &Window) -> &Before {
    window.user_data().insert_if_missing(Before::default);
    window.user_data().get::<Before>().expect("inserted above")
}

/// Where `window` was before it went fullscreen, if it is.
pub fn was(window: &Window) -> Option<Rectangle<i32, Logical>> {
    window.user_data().get::<Before>()?.0.get()
}

impl State {
    /// The whole screen, in the space's points: for fullscreen windows,
    /// absolute pointer positions and keeping the pointer on screen.
    pub fn screen(&self) -> Rectangle<i32, Logical> {
        self.space.output_geometry(&self.output).unwrap_or_default()
    }

    /// Whether `rect` is the whole screen: what a game that sizes itself
    /// asks for instead of saying "fullscreen".
    pub fn is_screen(&self, rect: Rectangle<i32, Logical>) -> bool {
        self.outputs
            .iter()
            .any(|o| self.space.output_geometry(o) == Some(rect))
    }

    /// The screen `window` is on, in the space's points.
    pub fn screen_of(&self, window: &Window) -> Rectangle<i32, Logical> {
        self.space
            .output_geometry(&self.output_of(window))
            .unwrap_or_else(|| self.screen())
    }

    /// Makes `window` cover the screen, remembering where it was.
    pub fn fullscreen(&mut self, window: &Window) {
        let screen = self.screen_of(window);
        if self.fullscreen.as_ref() != Some(window) {
            if let Some(other) = self.fullscreen.take() {
                self.unfullscreen(&other);
            }
            let location = self.space.element_location(window).unwrap_or(screen.loc);
            before(window)
                .0
                .set(Some(Rectangle::new(location, window.geometry().size)));
        }
        if let Some(toplevel) = window.toplevel() {
            toplevel.with_pending_state(|s| {
                s.states.set(xdg_toplevel::State::Fullscreen);
                s.size = Some(screen.size);
            });
            if toplevel.is_initial_configure_sent() {
                toplevel.send_pending_configure();
            }
        } else if let Some(x11) = window.x11_surface() {
            let _ = x11.set_fullscreen(true);
            let _ = x11.configure(screen);
        }
        self.fullscreen = Some(window.clone());
        self.place(window, screen.loc);
        self.focus_window(window.clone());
    }

    /// Puts a fullscreen `window` back where it was.
    pub fn unfullscreen(&mut self, window: &Window) {
        if self.fullscreen.as_ref() == Some(window) {
            self.fullscreen = None;
        }
        let Some(rect) = before(window).0.take() else {
            return;
        };
        if let Some(toplevel) = window.toplevel() {
            toplevel.with_pending_state(|s| {
                s.states.unset(xdg_toplevel::State::Fullscreen);
                s.size = Some(rect.size);
            });
            toplevel.send_pending_configure();
        } else if let Some(x11) = window.x11_surface() {
            let _ = x11.set_fullscreen(false);
            let _ = x11.configure(rect);
        }
        self.place(window, rect.loc);
        self.needs_redraw = true;
    }

    /// The fullscreen window, while it's the one with the keyboard; it's
    /// drawn over the bar and gets the pointer there.
    pub fn fullscreen_on_top(&self) -> Option<Window> {
        let window = self.fullscreen.as_ref()?;
        if !smithay::utils::IsAlive::alive(window) || self.locked() {
            return None;
        }
        (self.focused_window().as_ref() == Some(window)).then(|| window.clone())
    }

    /// The surface under `pos` in the fullscreen window on top, if any.
    pub fn fullscreen_under(
        &self,
        pos: Point<f64, Logical>,
    ) -> Option<(WlSurface, Point<f64, Logical>)> {
        let window = self.fullscreen_on_top()?;
        let origin = self.space.element_location(&window)?;
        window
            .surface_under(pos - origin.to_f64(), WindowSurfaceType::ALL)
            .map(|(s, p)| (s, (p + origin).to_f64()))
    }
}

impl State {
    /// The fullscreen window on top and where it is, for drawing.
    pub fn fullscreen_drawn(&self) -> Option<(Window, Point<i32, Logical>)> {
        let window = self.fullscreen_on_top()?;
        let origin = self.space.element_location(&window)?;
        Some((window, origin))
    }
}

/// The fullscreen window's elements, to draw over everything but the cursor.
pub fn elements<R>(
    renderer: &mut R,
    drawn: Option<&(Window, Point<i32, Logical>)>,
    scale: f64,
) -> Vec<WaylandSurfaceRenderElement<R>>
where
    R: Renderer + ImportAll,
    R::TextureId: Clone + Texture + 'static,
{
    let Some((window, origin)) = drawn else {
        return Vec::new();
    };
    let at = (*origin - window.geometry().loc).to_physical_precise_round(scale);
    window.render_elements(renderer, at, Scale::from(scale), 1.0)
}
