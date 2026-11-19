//! Looking at the picture: what the wheel and the touchpad do, and zooming about a point. No
//! egui widgets here, only the arithmetic; tested.
//!
//! The canvas is a scroll area whose content is the picture at its zoom with [`MARGIN`] round it,
//! in the middle of the content when that's smaller than the window. `scroll` is how far the
//! content has gone up and left.

use egui::{Modifiers, Vec2};

/// The least and most the picture can be zoomed: 5% to 6400%.
pub const MIN_ZOOM: f32 = 0.05;
pub const MAX_ZOOM: f32 = 64.0;

/// Room round the picture, in screen pixels.
pub const MARGIN: f32 = 12.0;

/// How much one notch of a mouse wheel zooms by.
const NOTCH: f32 = 1.2;

/// How far one notch pans, in screen pixels.
const NOTCH_PIXELS: f32 = 60.0;

/// What a turn of the wheel or a swipe on a touchpad asks for.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Gesture {
    /// Zoom by this factor, about the pointer.
    Zoom(f32),
    /// Move the view by this, added to how far it has scrolled.
    Pan(Vec2),
}

/// What a scroll of `delta` (up and left are positive, as egui reports it) with `modifiers` does.
/// `notched` is a mouse wheel, which turns in steps; a touchpad scrolls smoothly, in pixels.
///
/// - the wheel zooms; Ctrl and the wheel zooms too, as in most editors;
/// - Shift and the wheel pans sideways, Alt and the wheel up and down;
/// - a touchpad's two-finger swipe pans, and with Ctrl it zooms.
pub fn gesture(notched: bool, delta: Vec2, modifiers: Modifiers) -> Gesture {
    let pixels = if notched { NOTCH_PIXELS } else { 1.0 };
    let zoom = |amount: f32| Gesture::Zoom(if notched { NOTCH.powf(amount) } else { (amount * 0.01).exp() });
    if modifiers.ctrl || modifiers.command {
        zoom(delta.y + delta.x)
    } else if modifiers.shift {
        // Some systems turn Shift and the wheel into sideways scrolling already.
        Gesture::Pan(Vec2::new(-(delta.x + delta.y) * pixels, 0.0))
    } else if modifiers.alt {
        Gesture::Pan(Vec2::new(0.0, -(delta.y + delta.x) * pixels))
    } else if notched {
        zoom(delta.y)
    } else {
        Gesture::Pan(-delta * pixels)
    }
}

/// The content's size: the picture at `zoom` with its margin, and at least the window.
fn content(zoom: f32, window: Vec2, picture: Vec2) -> Vec2 {
    (picture * zoom + Vec2::splat(MARGIN * 2.0)).max(window)
}

/// The zoom and scroll after zooming by `factor` so the point of the picture under `anchor` (in
/// the window, from its top left) stays under it, as far as the scroll can go: it's kept within
/// the content, so a picture that now fits has none. `picture` is its size in its own pixels. The
/// zoom is kept within its limits.
pub fn zoom_about(zoom: f32, scroll: Vec2, factor: f32, anchor: Vec2, window: Vec2, picture: Vec2) -> (f32, Vec2) {
    let next = (zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
    if next == zoom {
        return (zoom, scroll);
    }
    // Where the picture's top left is in the content.
    let origin = |z: f32| (content(z, window, picture) - picture * z) / 2.0;
    let under = (scroll + anchor - origin(zoom)) / zoom;
    let most = (content(next, window, picture) - window).max(Vec2::ZERO);
    let scroll = (origin(next) + under * next - anchor).clamp(Vec2::ZERO, most);
    (next, scroll)
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::vec2;

    const NONE: Modifiers = Modifiers::NONE;

    fn is_zoom(g: Gesture) -> f32 {
        match g {
            Gesture::Zoom(f) => f,
            other => panic!("not a zoom: {other:?}"),
        }
    }

    fn is_pan(g: Gesture) -> Vec2 {
        match g {
            Gesture::Pan(p) => p,
            other => panic!("not a pan: {other:?}"),
        }
    }

    #[test]
    fn the_wheel_zooms_in_going_up_and_out_going_down() {
        assert!(is_zoom(gesture(true, vec2(0.0, 1.0), NONE)) > 1.0);
        assert!(is_zoom(gesture(true, vec2(0.0, -1.0), NONE)) < 1.0);
        // A notch each way comes back to where it started.
        let there = is_zoom(gesture(true, vec2(0.0, 2.0), NONE));
        let back = is_zoom(gesture(true, vec2(0.0, -2.0), NONE));
        assert!((there * back - 1.0).abs() < 1e-5);
    }

    #[test]
    fn ctrl_and_the_wheel_zooms_the_same() {
        let plain = is_zoom(gesture(true, vec2(0.0, 1.0), NONE));
        assert_eq!(is_zoom(gesture(true, vec2(0.0, 1.0), Modifiers::CTRL)), plain);
        assert_eq!(is_zoom(gesture(true, vec2(0.0, 1.0), Modifiers::COMMAND)), plain);
    }

    #[test]
    fn shift_pans_sideways_and_alt_up_and_down() {
        // Wheel up shows what's to the left, or above: the scroll goes down.
        let left = is_pan(gesture(true, vec2(0.0, 1.0), Modifiers::SHIFT));
        assert!(left.x < 0.0 && left.y == 0.0, "{left:?}");
        let right = is_pan(gesture(true, vec2(0.0, -1.0), Modifiers::SHIFT));
        assert!(right.x > 0.0);
        // Where the system has already turned it sideways, it's the same.
        assert_eq!(is_pan(gesture(true, vec2(1.0, 0.0), Modifiers::SHIFT)), left);
        let up = is_pan(gesture(true, vec2(0.0, 1.0), Modifiers::ALT));
        assert!(up.y < 0.0 && up.x == 0.0, "{up:?}");
    }

    #[test]
    fn a_touchpad_pans_both_ways_and_zooms_with_ctrl() {
        let pan = is_pan(gesture(false, vec2(10.0, -4.0), NONE));
        assert_eq!(pan, vec2(-10.0, 4.0), "the picture follows the fingers");
        assert!(is_zoom(gesture(false, vec2(0.0, 20.0), Modifiers::CTRL)) > 1.0);
        assert!(is_zoom(gesture(false, vec2(0.0, -20.0), Modifiers::CTRL)) < 1.0);
    }

    #[test]
    fn zooming_keeps_the_point_under_the_pointer() {
        let (window, picture) = (vec2(400.0, 300.0), vec2(1000.0, 800.0));
        let under = |zoom: f32, scroll: Vec2, anchor: Vec2| {
            let size = picture * zoom;
            let origin = ((size + Vec2::splat(MARGIN * 2.0)).max(window) - size) / 2.0;
            (scroll + anchor - origin) / zoom
        };
        let (zoom, scroll, anchor) = (1.0, vec2(100.0, 50.0), vec2(200.0, 150.0));
        for factor in [2.0, 1.5, 0.8] {
            let (z, s) = zoom_about(zoom, scroll, factor, anchor, window, picture);
            assert!((z - factor).abs() < 1e-5);
            let (a, b) = (under(zoom, scroll, anchor), under(z, s, anchor));
            assert!((a - b).length() < 0.01, "{factor}: {a:?} vs {b:?}");
        }
    }

    #[test]
    fn zooming_has_limits_and_a_picture_that_fits_is_not_scrolled() {
        let (window, picture) = (vec2(400.0, 300.0), vec2(100.0, 80.0));
        let (z, _) = zoom_about(MAX_ZOOM, Vec2::ZERO, 2.0, vec2(10.0, 10.0), window, picture);
        assert_eq!(z, MAX_ZOOM);
        let (z, _) = zoom_about(MIN_ZOOM, Vec2::ZERO, 0.5, vec2(10.0, 10.0), window, picture);
        assert_eq!(z, MIN_ZOOM);
        // Zoomed out until it's smaller than the window, nothing is left to scroll.
        let (_, scroll) = zoom_about(2.0, vec2(30.0, 20.0), 0.25, vec2(200.0, 150.0), window, picture);
        assert_eq!(scroll, Vec2::ZERO);
    }
}
