//! The colour picker's spectrum: one strip to point at, in place of a palette of swatches.
//!
//! Across it runs the hue, from red round to red; down it, a colour goes from white at the top
//! through its full strength in the middle to black at the bottom. A narrow column at the right
//! end is the greys, white to black. Both ends keep a flat band, so white and black are easy to
//! land on exactly. [`colour`] is the one place the layout lives: the image is made from it, and
//! so is what a click picks.

use egui::ColorImage;

use crate::bitmap::Rgba;

/// Where the greys begin, as a fraction of the width.
const GREYS: f32 = 0.94;
/// The flat white band at the top and the flat black one at the bottom, as fractions of the height.
const BAND: f32 = 0.06;
/// Where the hue is at its full strength, from and to.
const FULL: (f32, f32) = (0.45, 0.55);

/// The colour at (`u`, `v`), each from 0 (left, top) to 1 (right, bottom).
pub fn colour(u: f32, v: f32) -> Rgba {
    let (u, v) = (u.clamp(0.0, 1.0), v.clamp(0.0, 1.0));
    if u >= GREYS {
        let g = ((1.0 - v) * 255.0).round() as u8;
        return [g, g, g, 255];
    }
    let hue = u / GREYS;
    let saturation = ((v - BAND) / (FULL.0 - BAND)).clamp(0.0, 1.0);
    let value = (1.0 - (v - FULL.1) / (1.0 - BAND - FULL.1)).clamp(0.0, 1.0);
    hsv(hue, saturation, value)
}

/// `hue`, `saturation` and `value`, each 0 to 1, as an opaque colour.
fn hsv(hue: f32, saturation: f32, value: f32) -> Rgba {
    let h = (hue.rem_euclid(1.0)) * 6.0;
    let (sector, f) = (h.floor() as i32, h - h.floor());
    let (p, q, t) = (value * (1.0 - saturation), value * (1.0 - saturation * f), value * (1.0 - saturation * (1.0 - f)));
    let (r, g, b) = match sector {
        0 => (value, t, p),
        1 => (q, value, p),
        2 => (p, value, t),
        3 => (p, q, value),
        4 => (t, p, value),
        _ => (value, p, q),
    };
    let byte = |c: f32| (c * 255.0).round() as u8;
    [byte(r), byte(g), byte(b), 255]
}

/// The strip as an image, `width` by `height` pixels.
pub fn image(width: usize, height: usize) -> ColorImage {
    let mut pixels = Vec::with_capacity(width * height);
    for y in 0..height {
        for x in 0..width {
            let c = colour((x as f32 + 0.5) / width as f32, (y as f32 + 0.5) / height as f32);
            pixels.push(egui::Color32::from_rgb(c[0], c[1], c[2]));
        }
    }
    ColorImage::new([width, height], pixels)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn white_on_top_black_below_and_the_hue_between() {
        assert_eq!(colour(0.0, 0.0), [255, 255, 255, 255]);
        assert_eq!(colour(0.5, 0.02), [255, 255, 255, 255], "a flat band of white");
        assert_eq!(colour(0.5, 1.0), [0, 0, 0, 255]);
        assert_eq!(colour(0.5, 0.98), [0, 0, 0, 255], "a flat band of black");
        assert_eq!(colour(0.0, 0.5), [255, 0, 0, 255], "red at the left");
        assert_eq!(colour(GREYS / 3.0, 0.5), [0, 255, 0, 255], "green a third along");
        assert_eq!(colour(GREYS * 2.0 / 3.0, 0.5), [0, 0, 255, 255], "blue two thirds along");
    }

    #[test]
    fn lighter_above_the_middle_and_darker_below() {
        let (light, full, dark) = (colour(0.0, 0.25), colour(0.0, 0.5), colour(0.0, 0.75));
        assert!(light[1] > full[1] && light[2] > full[2], "paler above: {light:?}");
        assert!(dark[0] < full[0], "darker below: {dark:?}");
    }

    #[test]
    fn the_greys_run_white_to_black_at_the_right_end() {
        assert_eq!(colour(1.0, 0.0), [255, 255, 255, 255]);
        assert_eq!(colour(1.0, 1.0), [0, 0, 0, 255]);
        let middle = colour(0.97, 0.5);
        assert_eq!(middle[0], middle[1]);
        assert_eq!(middle[1], middle[2]);
        assert!((120..=136).contains(&middle[0]), "{middle:?}");
    }

    #[test]
    fn the_image_is_made_from_the_same_colours() {
        let image = image(100, 40);
        assert_eq!(image.size, [100, 40]);
        let c = colour(10.5 / 100.0, 20.5 / 40.0);
        assert_eq!(image.pixels[20 * 100 + 10], egui::Color32::from_rgb(c[0], c[1], c[2]));
    }
}
