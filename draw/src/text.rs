//! Text on the picture: letters rasterised into the bitmap, anti-aliased, in the font egui brings
//! (Ubuntu). A line's top is the `at` given, and `\n` starts the next line.

use std::sync::OnceLock;

use ab_glyph::{Font, FontRef, PxScale, ScaleFont, point};

use crate::bitmap::{Bitmap, Rgba};

fn font() -> &'static FontRef<'static> {
    static FONT: OnceLock<FontRef<'static>> = OnceLock::new();
    FONT.get_or_init(|| FontRef::try_from_slice(epaint_default_fonts::UBUNTU_LIGHT).expect("a font egui ships"))
}

/// How tall the letters are for a pen size: text wants more room than a line does.
pub fn pixels(size: u32) -> f32 {
    (size as f32 * 5.0).max(10.0)
}

/// Draws `text` with its top left at `at`, `px` pixels from line to line's ascent to descent, in
/// `colour`. A little heavier than the font's light weight: marking up a picture has to be read.
pub fn draw(bitmap: &mut Bitmap, at: (i32, i32), px: f32, text: &str, colour: Rgba) {
    let scaled = font().as_scaled(PxScale::from(px));
    let line_height = scaled.height() + scaled.line_gap();
    // Drawn twice, a fraction of the size apart, which thickens the strokes.
    let heft = (px / 28.0).max(0.4);
    for (n, line) in text.lines().enumerate() {
        let baseline = at.1 as f32 + scaled.ascent() + n as f32 * line_height;
        for offset in [0.0, heft] {
            let mut x = at.0 as f32 + offset;
            let mut previous = None;
            for c in line.chars() {
                let id = scaled.glyph_id(c);
                if let Some(before) = previous {
                    x += scaled.kern(before, id);
                }
                let glyph = id.with_scale_and_position(scaled.scale(), point(x, baseline));
                x += scaled.h_advance(id);
                previous = Some(id);
                let Some(outline) = font().outline_glyph(glyph) else { continue };
                let bounds = outline.px_bounds();
                outline.draw(|gx, gy, coverage| {
                    let (px, py) = (bounds.min.x as i32 + gx as i32, bounds.min.y as i32 + gy as i32);
                    if let Some(under) = bitmap.get(px, py) {
                        bitmap.put(px, py, blend(under, colour, coverage));
                    }
                });
            }
        }
    }
}

/// `colour` over `under`, by how much of the pixel the letter covers.
fn blend(under: Rgba, colour: Rgba, coverage: f32) -> Rgba {
    let c = coverage.clamp(0.0, 1.0);
    let mix = |u: u8, t: u8| (u as f32 + (t as f32 - u as f32) * c).round() as u8;
    [mix(under[0], colour[0]), mix(under[1], colour[1]), mix(under[2], colour[2]), 255]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitmap::WHITE;

    const BLACK: Rgba = [0, 0, 0, 255];

    fn ink(b: &Bitmap) -> usize {
        b.pixels.iter().filter(|p| **p != WHITE).count()
    }

    #[test]
    fn text_lands_at_its_corner_and_leaves_the_rest() {
        let mut b = Bitmap::new(200, 80, WHITE);
        draw(&mut b, (20, 10), 24.0, "Hi", BLACK);
        assert!(ink(&b) > 40);
        let inked = |x0: i32, y0: i32, x1: i32, y1: i32| {
            (y0..y1).any(|y| (x0..x1).any(|x| b.get(x, y) != Some(WHITE)))
        };
        assert!(inked(20, 10, 60, 40), "in its box");
        assert!(!inked(0, 0, 18, 80), "nothing left of it");
        assert!(!inked(0, 50, 200, 80), "nothing below it");
        assert!(b.pixels.iter().any(|p| *p != WHITE && *p != BLACK), "anti-aliased edges");
    }

    #[test]
    fn a_second_line_is_lower_and_bigger_text_takes_more() {
        let mut one = Bitmap::new(200, 120, WHITE);
        let mut two = Bitmap::new(200, 120, WHITE);
        draw(&mut one, (5, 5), 20.0, "ab", BLACK);
        draw(&mut two, (5, 5), 20.0, "ab\ncd", BLACK);
        assert!(ink(&two) > ink(&one) + 30);
        let mut big = Bitmap::new(200, 120, WHITE);
        draw(&mut big, (5, 5), 40.0, "ab", BLACK);
        assert!(ink(&big) > ink(&one) * 2);
    }

    #[test]
    fn text_off_the_edge_is_cut_and_nothing_panics() {
        let mut b = Bitmap::new(30, 20, WHITE);
        draw(&mut b, (-10, -5), 30.0, "Wide text", BLACK);
        draw(&mut b, (200, 200), 30.0, "gone", BLACK);
        draw(&mut b, (0, 0), 30.0, "", BLACK);
        assert!(ink(&b) > 0);
    }
}
