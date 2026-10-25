//! Dragging out a part of the screen to take (ADR-0091) or record
//! (ADR-0092), after Selection or Record Selection in the Print key's
//! choices: the screen dims under a crosshair, the part dragged out shows
//! through with a white edge, and letting go takes it or starts recording.
//! Escape, another button, or a click without a drag takes nothing. The
//! compositor draws it all, with the clicks and the pointer kept from the
//! windows meanwhile.

use smithay::{
    backend::renderer::element::{
        Kind,
        solid::{SolidColorBuffer, SolidColorRenderElement},
    },
    input::pointer::CursorIcon,
    utils::{Logical, Point, Rectangle},
};

use crate::state::State;

/// Premultiplied: black over what isn't taken, and the selection's edge.
const DIM: [f32; 4] = [0.0, 0.0, 0.0, 0.35];
const EDGE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

/// What a selection is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    Screenshot,
    Recording,
}

/// A selection being made.
pub struct Selecting {
    purpose: Purpose,
    /// Where the left button went down, once it has.
    from: Option<Point<f64, Logical>>,
    /// Where the pointer is.
    to: Point<f64, Logical>,
    /// One per piece drawn, kept so they're only redrawn as they change.
    buffers: Vec<SolidColorBuffer>,
}

/// The rectangle with `a` and `b` at opposite corners, out to whole pixels.
fn between(a: Point<f64, Logical>, b: Point<f64, Logical>) -> Rectangle<i32, Logical> {
    let (left, top) = (a.x.min(b.x).floor() as i32, a.y.min(b.y).floor() as i32);
    let (right, bottom) = (a.x.max(b.x).ceil() as i32, a.y.max(b.y).ceil() as i32);
    Rectangle::new((left, top).into(), (right - left, bottom - top).into())
}

/// What's drawn over `screen`: all of it dimmed before a drag; then the dim
/// around `selected` and its edge, inside it.
fn pieces(
    screen: Rectangle<i32, Logical>,
    selected: Option<Rectangle<i32, Logical>>,
) -> Vec<(Rectangle<i32, Logical>, [f32; 4])> {
    let Some(s) = selected.and_then(|s| s.intersection(screen)) else {
        return vec![(screen, DIM)];
    };
    let rect = |x, y, w, h| Rectangle::<i32, Logical>::new((x, y).into(), (w, h).into());
    let (left, top) = (screen.loc.x, screen.loc.y);
    let (right, bottom) = (left + screen.size.w, top + screen.size.h);
    let (s_right, s_bottom) = (s.loc.x + s.size.w, s.loc.y + s.size.h);
    [
        (rect(left, top, screen.size.w, s.loc.y - top), DIM),
        (rect(left, s_bottom, screen.size.w, bottom - s_bottom), DIM),
        (rect(left, s.loc.y, s.loc.x - left, s.size.h), DIM),
        (rect(s_right, s.loc.y, right - s_right, s.size.h), DIM),
        (rect(s.loc.x, s.loc.y, s.size.w, 1), EDGE),
        (rect(s.loc.x, s_bottom - 1, s.size.w, 1), EDGE),
        (rect(s.loc.x, s.loc.y + 1, 1, s.size.h - 2), EDGE),
        (rect(s_right - 1, s.loc.y + 1, 1, s.size.h - 2), EDGE),
    ]
    .into_iter()
    .filter(|(r, _)| r.size.w > 0 && r.size.h > 0)
    .collect()
}

impl State {
    /// Starts a selection, at the pointer.
    pub fn start_selecting(&mut self, purpose: Purpose) {
        if self.locked() {
            return;
        }
        let to = self
            .seat
            .get_pointer()
            .map(|p| p.current_location())
            .unwrap_or_default();
        self.shots.selecting = Some(Selecting {
            purpose,
            from: None,
            to,
            buffers: Vec::new(),
        });
        self.grab_cursor = Some(CursorIcon::Crosshair);
        self.needs_redraw = true;
    }

    /// Ends the selection without taking anything.
    pub fn cancel_selecting(&mut self) {
        if self.shots.selecting.take().is_some() {
            self.end_grab_cursor();
        }
    }

    /// The pointer moved to `pos`: true if a selection took it.
    pub fn select_moved(&mut self, pos: Point<f64, Logical>) -> bool {
        if self.locked() {
            self.cancel_selecting();
        }
        let Some(selecting) = self.shots.selecting.as_mut() else {
            return false;
        };
        selecting.to = pos;
        self.grab_cursor = Some(CursorIcon::Crosshair);
        self.needs_redraw = true;
        true
    }

    /// A button at `at`: true if a selection took it.
    pub fn select_button(&mut self, button: u32, pressed: bool, at: Point<f64, Logical>) -> bool {
        if self.locked() {
            self.cancel_selecting();
        }
        let Some(selecting) = self.shots.selecting.as_mut() else {
            return false;
        };
        selecting.to = at;
        if button != crate::grabs::BTN_LEFT {
            if pressed {
                self.cancel_selecting();
            }
            return true;
        }
        match (pressed, selecting.from) {
            (true, _) => selecting.from = Some(at),
            (false, Some(from)) => {
                let (area, purpose) = (between(from, at), selecting.purpose);
                self.cancel_selecting();
                if area.size.w >= 2 && area.size.h >= 2 {
                    match purpose {
                        Purpose::Screenshot => self.take_screenshot(None, Some(area)),
                        Purpose::Recording => self.start_recording(Some(area)),
                    }
                }
            }
            (false, None) => {}
        }
        self.needs_redraw = true;
        true
    }

    /// The dim and the edge for this frame, while selecting.
    pub fn selection_elements(&mut self) -> Vec<SolidColorRenderElement> {
        let Some(screen) = self.space.output_geometry(&self.output) else {
            return Vec::new();
        };
        let scale = self.output.current_scale().fractional_scale();
        let Some(selecting) = self.shots.selecting.as_mut() else {
            return Vec::new();
        };
        let selected = selecting.from.map(|from| between(from, selecting.to));
        let pieces = pieces(screen, selected);
        selecting
            .buffers
            .resize_with(pieces.len(), SolidColorBuffer::default);
        pieces
            .into_iter()
            .zip(&mut selecting.buffers)
            .map(|((area, colour), buffer)| {
                buffer.update(area.size, colour);
                SolidColorRenderElement::from_buffer(
                    buffer,
                    (area.loc - screen.loc).to_physical_precise_round(scale),
                    scale,
                    1.0,
                    Kind::Unspecified,
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: i32, y: i32, w: i32, h: i32) -> Rectangle<i32, Logical> {
        Rectangle::new((x, y).into(), (w, h).into())
    }

    #[test]
    fn either_way_dragged_is_the_same_rectangle() {
        let a = Point::from((10.4, 20.6));
        let b = Point::from((5.5, 30.2));
        assert_eq!(between(a, b), rect(5, 20, 6, 11));
        assert_eq!(between(b, a), rect(5, 20, 6, 11));
    }

    #[test]
    fn before_a_drag_the_whole_screen_dims() {
        let screen = rect(0, 0, 100, 50);
        assert_eq!(pieces(screen, None), vec![(screen, DIM)]);
    }

    #[test]
    fn the_selection_shows_through_with_an_edge() {
        let screen = rect(0, 0, 100, 50);
        let got = pieces(screen, Some(rect(10, 10, 20, 20)));
        let dimmed: i32 = got
            .iter()
            .filter(|(_, c)| *c == DIM)
            .map(|(r, _)| r.size.w * r.size.h)
            .sum();
        assert_eq!(dimmed, 100 * 50 - 20 * 20, "everything but the selection");
        let edge: i32 = got
            .iter()
            .filter(|(_, c)| *c == EDGE)
            .map(|(r, _)| r.size.w * r.size.h)
            .sum();
        assert_eq!(edge, 20 * 20 - 18 * 18, "a pixel inside it");
    }

    #[test]
    fn a_selection_to_the_screens_edge_leaves_no_empty_pieces() {
        let screen = rect(0, 0, 100, 50);
        let got = pieces(screen, Some(rect(-5, -5, 50, 200)));
        assert!(got.iter().all(|(r, _)| r.size.w > 0 && r.size.h > 0));
        assert_eq!(
            got.iter().filter(|(_, c)| *c == DIM).count(),
            1,
            "the right"
        );
    }
}
