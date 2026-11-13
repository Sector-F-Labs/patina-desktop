//! Minimising and restoring, shown (ADR-0136): a window that's minimised
//! shrinks and fades as it goes to the window list, wherever that is on
//! screen, and one that's brought back grows and fades in from it. While it
//! goes the window is drawn as a ghost, the way it was, scaled about its top
//! left corner and moved along, so a minimise that happens all at once still
//! takes the same time.

use std::time::{Duration, Instant};

use smithay::{
    backend::renderer::{
        ImportAll, ImportMem, Renderer, Texture,
        element::{
            AsRenderElements, Element,
            surface::WaylandSurfaceRenderElement,
            utils::{Relocate, RelocateRenderElement, RescaleRenderElement},
        },
    },
    desktop::{Window, layer_map_for_output},
    output::Output,
    utils::{Logical, Physical, Point, Rectangle, Scale},
};

use crate::frames::{self, DesktopElement, TITLE};
use crate::state::State;

/// How long a window takes to go or come.
const DURATION: Duration = Duration::from_millis(280);
/// How much of its size it loses on the way: at the window list it's a sixth.
const SHRINK: f64 = 5.0 / 6.0;

/// A window on its way to the window list, or back from it.
pub struct Ghost {
    window: Window,
    /// Where the window's own surface is, in the space.
    location: Point<i32, Logical>,
    /// The window with its title bar, if it has one.
    bounds: Rectangle<i32, Logical>,
    framed: bool,
    /// Where the window list is: the middle of it, in the space.
    to: Point<f64, Logical>,
    start: Instant,
    /// Coming back: the same way round, backwards.
    restoring: bool,
}

impl Ghost {
    pub fn window(&self) -> &Window {
        &self.window
    }

    fn done(&self, now: Instant) -> bool {
        now.duration_since(self.start) >= DURATION
    }

    /// How far it is from where the window is: 0 at the window, 1 gone at
    /// the window list, eased at both ends.
    fn away(&self, now: Instant) -> f64 {
        let t =
            (now.duration_since(self.start).as_secs_f64() / DURATION.as_secs_f64()).clamp(0.0, 1.0);
        let eased = if t < 0.5 {
            4.0 * t * t * t
        } else {
            1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
        };
        if self.restoring { 1.0 - eased } else { eased }
    }
}

/// `element` shrunk by `shrink` about `origin`, then moved by `by`.
fn place<E: Element>(
    element: E,
    origin: Point<i32, Physical>,
    shrink: Scale<f64>,
    by: Point<i32, Physical>,
) -> RelocateRenderElement<RescaleRenderElement<E>> {
    RelocateRenderElement::from_element(
        RescaleRenderElement::from_element(element, origin, shrink),
        by,
        Relocate::Relative,
    )
}

/// `ghosts` drawn on the output whose top left is `screen`, scaled by
/// `scale` into pixels: the window's surfaces and its frame, shrunk toward
/// the window list and faded by how far it has gone.
pub fn elements<R>(
    renderer: &mut R,
    ghosts: &[Ghost],
    screen: Point<i32, Logical>,
    scale: f64,
) -> Vec<DesktopElement<R>>
where
    R: Renderer + ImportAll + ImportMem,
    R::TextureId: Send + Clone + Texture + 'static,
{
    let now = Instant::now();
    let mut out = Vec::new();
    for ghost in ghosts {
        let away = ghost.away(now);
        let shrink = 1.0 - SHRINK * away;
        let alpha = (1.0 - away) as f32;
        let (w, h) = (ghost.bounds.size.w as f64, ghost.bounds.size.h as f64);
        let top_left = (
            (ghost.bounds.loc.x - screen.x) as f64,
            (ghost.bounds.loc.y - screen.y) as f64,
        );
        let middle = (top_left.0 + w / 2.0, top_left.1 + h / 2.0);
        let to = (ghost.to.x - screen.x as f64, ghost.to.y - screen.y as f64);
        let centre = (
            middle.0 + (to.0 - middle.0) * away,
            middle.1 + (to.1 - middle.1) * away,
        );
        let moved = (
            centre.0 - w * shrink / 2.0 - top_left.0,
            centre.1 - h * shrink / 2.0 - top_left.1,
        );
        let origin: Point<i32, Physical> = Point::<f64, Logical>::from(top_left)
            .to_physical(scale)
            .to_i32_round();
        let by: Point<i32, Physical> = Point::<f64, Logical>::from(moved)
            .to_physical(scale)
            .to_i32_round();
        let scale_by = Scale::from(shrink);

        let location = ghost.location - screen;
        if ghost.framed
            && let Some(frame) =
                frames::frame_element(renderer, &ghost.window, location, scale, alpha)
        {
            out.push(DesktopElement::GhostFrame(place(
                frame, origin, scale_by, by,
            )));
        }
        let at = (location - ghost.window.geometry().loc).to_physical_precise_round(scale);
        out.extend(
            ghost
                .window
                .render_elements::<WaylandSurfaceRenderElement<R>>(
                    renderer,
                    at,
                    Scale::from(scale),
                    alpha,
                )
                .into_iter()
                .map(|surface| DesktopElement::Ghost(place(surface, origin, scale_by, by))),
        );
    }
    out
}

impl State {
    /// Starts showing `window`, at `location` in the space, going to the
    /// window list, or `restoring`, coming from it.
    pub fn start_ghost(&mut self, window: &Window, location: Point<i32, Logical>, restoring: bool) {
        let size = window.geometry().size;
        if size.w <= 0 || size.h <= 0 {
            return;
        }
        let framed = frames::wants_frame(window) && self.fullscreen.as_ref() != Some(window);
        let bounds = if framed {
            Rectangle::new(
                (location.x, location.y - TITLE).into(),
                (size.w, size.h + TITLE).into(),
            )
        } else {
            Rectangle::new(location, size)
        };
        let to = self.window_list_middle(bounds);
        self.ghosts.retain(|g| g.window != *window);
        self.ghosts.push(Ghost {
            window: window.clone(),
            location,
            bounds,
            framed,
            to,
            start: Instant::now(),
            restoring,
        });
        self.needs_redraw = true;
    }

    /// The middle of the window list on the monitor `bounds` is on: its
    /// layer surface, wherever the HUD puts it; the bottom of the screen
    /// where there's none (the HUD isn't running, or hides it).
    fn window_list_middle(&self, bounds: Rectangle<i32, Logical>) -> Point<f64, Logical> {
        let middle = Point::<f64, Logical>::from((
            bounds.loc.x as f64 + bounds.size.w as f64 / 2.0,
            bounds.loc.y as f64 + bounds.size.h as f64 / 2.0,
        ));
        let output = self.output_under(middle);
        let screen = self.space.output_geometry(output).unwrap_or_default();
        match self.window_list(output) {
            Some(list) => (
                list.loc.x as f64 + list.size.w as f64 / 2.0,
                list.loc.y as f64 + list.size.h as f64 / 2.0,
            )
                .into(),
            None => (
                screen.loc.x as f64 + screen.size.w as f64 / 2.0,
                (screen.loc.y + screen.size.h) as f64,
            )
                .into(),
        }
    }

    /// The window list's layer surface on `output`, in the space.
    fn window_list(&self, output: &Output) -> Option<Rectangle<i32, Logical>> {
        let screen = self.space.output_geometry(output)?;
        let layers = layer_map_for_output(output);
        ["patina-hud-windows", "patina-hud"]
            .into_iter()
            .find_map(|name| {
                layers
                    .layers()
                    .filter(|l| l.namespace() == name)
                    .find_map(|l| layers.layer_geometry(l))
            })
            .map(|g| Rectangle::new(screen.loc + g.loc, g.size))
    }

    /// Which way the edge the window list is attached to is on `output`, as
    /// a step along an axis: (1, 0) the right, (-1, 0) the left, (0, 1) the
    /// bottom, (0, -1) the top. A list that's taller than it is wide is on the
    /// left or right, whichever side it's nearer, and one that's wider on the
    /// top or bottom, wherever along the edge it sits (a corner's column or
    /// row counts as its own edge). The bottom where there's none.
    pub fn window_list_direction(&self, output: &Output) -> (f64, f64) {
        let (Some(list), Some(screen)) =
            (self.window_list(output), self.space.output_geometry(output))
        else {
            return (0.0, 1.0);
        };
        edge_of(list, screen)
    }

    /// Drops the ghosts that have arrived, and asks for another frame while
    /// any is still on its way. Called with each frame drawn.
    pub fn tick_ghosts(&mut self) {
        let now = Instant::now();
        self.ghosts.retain(|g| !g.done(now));
        if !self.ghosts.is_empty() {
            self.needs_redraw = true;
        }
    }
}

/// The edge of `screen` that `list`, a bar on it, is attached to, as a step
/// toward it; see [`State::window_list_direction`].
fn edge_of(list: Rectangle<i32, Logical>, screen: Rectangle<i32, Logical>) -> (f64, f64) {
    let (cx, cy) = (
        list.loc.x as f64 + list.size.w as f64 / 2.0 - screen.loc.x as f64,
        list.loc.y as f64 + list.size.h as f64 / 2.0 - screen.loc.y as f64,
    );
    let (w, h) = (screen.size.w as f64, screen.size.h as f64);
    if list.size.h > list.size.w {
        if cx < w / 2.0 {
            (-1.0, 0.0)
        } else {
            (1.0, 0.0)
        }
    } else if cy < h / 2.0 {
        (0.0, -1.0)
    } else {
        (0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_list_is_on_the_edge_it_lies_along() {
        let screen = Rectangle::<i32, Logical>::new((0, 0).into(), (1920, 1080).into());
        let at = |x, y, w, h| Rectangle::<i32, Logical>::new((x, y).into(), (w, h).into());
        assert_eq!(edge_of(at(400, 1040, 800, 40), screen), (0.0, 1.0));
        assert_eq!(edge_of(at(400, 0, 800, 40), screen), (0.0, -1.0));
        assert_eq!(edge_of(at(1880, 200, 40, 600), screen), (1.0, 0.0));
        assert_eq!(
            edge_of(at(1880, 900, 40, 150), screen),
            (1.0, 0.0),
            "a column low on the right"
        );
        assert_eq!(edge_of(at(0, 0, 40, 500), screen), (-1.0, 0.0));
    }

    #[test]
    fn the_size_a_window_shrinks_to_is_a_sixth() {
        assert!((1.0 - SHRINK - 1.0 / 6.0).abs() < 1e-9);
        assert!(DURATION > Duration::ZERO);
    }
}
