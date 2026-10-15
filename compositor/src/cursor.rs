//! The pointer's look (ADR-0032): Patina's own cursors, drawn from vector
//! shapes at the exact pixel size for the output's scale, so they're sharp
//! at 1.2 as at 2: a near-black body with a crisp white edge and a soft
//! shadow, rounded where lines meet. Clients ask for shapes by name (the
//! cursor-shape protocol, or the compositor while it moves or resizes a
//! window), and get them in this style; `[cursor] theme` swaps in an
//! installed cursor theme instead.

use std::collections::HashMap;

use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::element::memory::MemoryRenderBuffer;
use smithay::input::pointer::CursorIcon;
use smithay::utils::{Logical, Physical, Point, Size, Transform};

/// The body's colour, and the edge around it.
const INK: &str = "#15171b";
const EDGE: &str = "#ffffff";

/// The shapes Patina draws; every name a client can ask for maps to one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Shape {
    Arrow,
    Hand,
    Text,
    ResizeEw,
    ResizeNs,
    ResizeNwse,
    ResizeNesw,
    Move,
    Crosshair,
    NotAllowed,
    Wait,
    Progress,
}

impl Shape {
    #[cfg(test)]
    const ALL: [Shape; 12] = [
        Shape::Arrow,
        Shape::Hand,
        Shape::Text,
        Shape::ResizeEw,
        Shape::ResizeNs,
        Shape::ResizeNwse,
        Shape::ResizeNesw,
        Shape::Move,
        Shape::Crosshair,
        Shape::NotAllowed,
        Shape::Wait,
        Shape::Progress,
    ];

    fn of(icon: CursorIcon) -> Self {
        use CursorIcon as C;
        match icon {
            C::Pointer | C::Grab => Shape::Hand,
            C::Text | C::VerticalText => Shape::Text,
            C::EResize | C::WResize | C::EwResize | C::ColResize => Shape::ResizeEw,
            C::NResize | C::SResize | C::NsResize | C::RowResize => Shape::ResizeNs,
            C::NwResize | C::SeResize | C::NwseResize => Shape::ResizeNwse,
            C::NeResize | C::SwResize | C::NeswResize => Shape::ResizeNesw,
            C::Move | C::AllScroll | C::AllResize | C::Grabbing => Shape::Move,
            C::Crosshair | C::Cell => Shape::Crosshair,
            C::NotAllowed | C::NoDrop => Shape::NotAllowed,
            C::Wait => Shape::Wait,
            C::Progress => Shape::Progress,
            _ => Shape::Arrow,
        }
    }

    /// The shape on the 32-unit grid, and its hotspot there.
    fn drawing(self) -> (String, (f32, f32)) {
        let centre = (16.0, 16.0);
        match self {
            Shape::Arrow => (arrow(), (8.0, 5.0)),
            Shape::Hand => (filled(HAND), (14.5, 5.0)),
            Shape::Text => (lines(TEXT, 1.8), centre),
            Shape::ResizeEw => (filled(&rotated(DOUBLE_ARROW, 0)), centre),
            Shape::ResizeNs => (filled(&rotated(DOUBLE_ARROW, 90)), centre),
            Shape::ResizeNwse => (filled(&rotated(DOUBLE_ARROW, 45)), centre),
            Shape::ResizeNesw => (filled(&rotated(DOUBLE_ARROW, -45)), centre),
            Shape::Move => (filled(MOVE), centre),
            Shape::Crosshair => (lines(CROSSHAIR, 1.6), centre),
            Shape::NotAllowed => (lines(NOT_ALLOWED, 2.6), centre),
            Shape::Wait => (spinner(16.0, 16.0, 8.0), centre),
            Shape::Progress => (
                format!("{}{}", arrow(), spinner(24.0, 24.0, 4.6)),
                (8.0, 5.0),
            ),
        }
    }
}

/// The arrow, leaning a little, with a notched tail.
fn arrow() -> String {
    filled(r#"<path d="M8 5 L8 25.2 L12.9 20.6 L16.3 27.8 L19.5 26.4 L16.2 19.4 L22.8 19.4 Z"/>"#)
}

/// A pointing hand: one finger up, three folded, and a thumb.
const HAND: &str = r#"<rect x="12.2" y="5" width="4.6" height="15" rx="2.3"/>
<rect x="16.6" y="12.2" width="4.4" height="9" rx="2.2"/>
<rect x="20.6" y="13.6" width="4.2" height="8.4" rx="2.1"/>
<rect x="12" y="16.5" width="12.8" height="11.5" rx="4.6"/>
<rect x="7.4" y="14.6" width="4.6" height="10" rx="2.3" transform="rotate(-32 9.7 19.6)"/>
<rect x="10.4" y="19" width="6" height="8.6" rx="3"/>"#;

/// The I-beam, with curved serifs.
const TEXT: &str = "M12.6 6.5 Q16 6.5 16 9.2 Q16 6.5 19.4 6.5 M16 9.2 V22.8 M12.6 25.5 Q16 25.5 16 22.8 Q16 25.5 19.4 25.5";

/// A two-headed arrow across the middle, left to right.
const DOUBLE_ARROW: &str = "M5 16 L10.8 10.4 L10.8 14.1 L21.2 14.1 L21.2 10.4 L27 16 L21.2 21.6 L21.2 17.9 L10.8 17.9 L10.8 21.6 Z";

/// Four arrows from the middle.
const MOVE: &str = r#"<path d="M4.5 16 L9.4 11.4 L9.4 14.4 L22.6 14.4 L22.6 11.4 L27.5 16 L22.6 20.6 L22.6 17.6 L9.4 17.6 L9.4 20.6 Z"/>
<path d="M16 4.5 L20.6 9.4 L17.6 9.4 L17.6 22.6 L20.6 22.6 L16 27.5 L11.4 22.6 L14.4 22.6 L14.4 9.4 L11.4 9.4 Z"/>"#;

/// A plus with a gap at the hotspot, and a dot in it.
const CROSSHAIR: &str = "M16 5 V12.5 M16 19.5 V27 M5 16 H12.5 M19.5 16 H27 M16 16 L16 16";

/// A circle, struck through.
const NOT_ALLOWED: &str = "M25 16 A9 9 0 1 1 7 16 A9 9 0 1 1 25 16 Z M9.6 22.4 L22.4 9.6";

fn rotated(path: &str, degrees: i32) -> String {
    format!(r#"<path d="{path}" transform="rotate({degrees} 16 16)"/>"#)
}

/// `shapes` as one body: their union in white, wider, then in ink, so the
/// edge goes round the outside only.
fn filled(shapes: &str) -> String {
    format!(
        r#"<g fill="{EDGE}" stroke="{EDGE}" stroke-width="3" stroke-linejoin="round">{shapes}</g>
<g fill="{INK}" stroke="{INK}" stroke-width="0.6" stroke-linejoin="round">{shapes}</g>"#
    )
}

/// The path `d` as strokes `width` wide, edged in white.
fn lines(d: &str, width: f32) -> String {
    let edge = width + 2.6;
    format!(
        r#"<path d="{d}" fill="none" stroke="{EDGE}" stroke-width="{edge}" stroke-linecap="round" stroke-linejoin="round"/>
<path d="{d}" fill="none" stroke="{INK}" stroke-width="{width}" stroke-linecap="round" stroke-linejoin="round"/>"#
    )
}

/// A busy ring at (`x`, `y`): three quarters of it drawn.
fn spinner(x: f32, y: f32, r: f32) -> String {
    let width = r * 0.42;
    let around = 2.0 * std::f32::consts::PI * r;
    format!(
        r#"<circle cx="{x}" cy="{y}" r="{r}" fill="none" stroke="{EDGE}" stroke-width="{edge}"/>
<circle cx="{x}" cy="{y}" r="{r}" fill="none" stroke="{INK}" stroke-width="{width}" stroke-opacity="0.25"/>
<circle cx="{x}" cy="{y}" r="{r}" fill="none" stroke="{INK}" stroke-width="{width}" stroke-linecap="round" stroke-dasharray="{dash} {around}" transform="rotate(-90 {x} {y})"/>"#,
        edge = width + 2.6,
        dash = around * 0.72,
    )
}

/// A whole cursor: the shape, with a soft shadow under it.
fn svg(body: &str) -> String {
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32" width="32" height="32">
<defs><filter id="shadow" x="-40%" y="-40%" width="180%" height="180%">
<feDropShadow dx="0" dy="0.9" stdDeviation="1" flood-color="#000" flood-opacity="0.4"/>
</filter></defs>
<g filter="url(#shadow)">{body}</g></svg>"##
    )
}

/// A cursor image ready for the renderer: premultiplied BGRA (DRM's
/// little-endian ARGB8888), its hotspot, and its size in pixels and in
/// logical units. The renderer scales logical sizes by the output's scale,
/// so the logical size is the one that scale maps back to the pixels
/// exactly, and the image is drawn 1:1, not resampled.
pub struct Image {
    pub buffer: MemoryRenderBuffer,
    pub hotspot: Point<i32, Physical>,
    pub pixels: Size<i32, Physical>,
    pub logical: Size<i32, Logical>,
}

/// Pixels `side` square, premultiplied BGRA, and the hotspot.
struct Pixels {
    side: u32,
    bgra: Vec<u8>,
    hotspot: (i32, i32),
}

fn draw(shape: Shape, side: u32) -> Option<Pixels> {
    let (body, (hx, hy)) = shape.drawing();
    let tree = resvg::usvg::Tree::from_str(&svg(&body), &resvg::usvg::Options::default()).ok()?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(side, side)?;
    let k = side as f32 / 32.0;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(k, k),
        &mut pixmap.as_mut(),
    );
    let bgra = pixmap
        .data()
        .chunks_exact(4)
        .flat_map(|p| [p[2], p[1], p[0], p[3]])
        .collect();
    Some(Pixels {
        side,
        bgra,
        hotspot: ((hx * k).round() as i32, (hy * k).round() as i32),
    })
}

/// The cursors at one size and scale, made as they're first asked for.
pub struct Cursors {
    /// An installed cursor theme to use instead, if any.
    theme: Option<String>,
    /// The cursor's size at scale 1, as XCURSOR_SIZE.
    size: u32,
    scale: f64,
    /// And in pixels at this scale.
    pixels: u32,
    made: HashMap<CursorIcon, Image>,
}

impl Cursors {
    /// `theme` is "" for Patina's own, or an installed theme's name;
    /// `size` the cursor's size at scale 1.
    pub fn new(theme: &str, size: u32, scale: f64) -> Self {
        Self {
            theme: (!theme.is_empty()).then(|| theme.to_string()),
            size: size.max(8),
            scale,
            pixels: ((size.max(8) as f64) * scale).round() as u32,
            made: HashMap::new(),
        }
    }

    pub fn get(&mut self, icon: CursorIcon) -> &Image {
        if !self.made.contains_key(&icon) {
            let image = self.make(icon);
            self.made.insert(icon, image);
        }
        &self.made[&icon]
    }

    fn make(&self, icon: CursorIcon) -> Image {
        if let Some(theme) = &self.theme
            && let Some(image) = themed(theme, icon, self.pixels, self.scale)
        {
            return image;
        }
        let pixels = draw(Shape::of(icon), self.pixels).unwrap_or(Pixels {
            side: 1,
            bgra: vec![0; 4],
            hotspot: (0, 0),
        });
        Image {
            buffer: MemoryRenderBuffer::from_slice(
                &pixels.bgra,
                Fourcc::Argb8888,
                (pixels.side as i32, pixels.side as i32),
                1,
                Transform::Normal,
                None,
            ),
            hotspot: pixels.hotspot.into(),
            pixels: (pixels.side as i32, pixels.side as i32).into(),
            // size * scale, rounded, is `pixels`: exactly 1:1.
            logical: (self.size as i32, self.size as i32).into(),
        }
    }
}

/// `icon` from the installed cursor theme `theme`, by its CSS name (or the
/// arrow's old X name), at the size nearest `pixels`.
fn themed(theme: &str, icon: CursorIcon, pixels: u32, scale: f64) -> Option<Image> {
    let theme = xcursor::CursorTheme::load(theme);
    let path = theme.load_icon(icon.name()).or_else(|| {
        (icon == CursorIcon::Default)
            .then(|| theme.load_icon("left_ptr"))
            .flatten()
    })?;
    let images = xcursor::parser::parse_xcursor(&std::fs::read(path).ok()?)?;
    let i = images
        .into_iter()
        .min_by_key(|i| (i.size as i64 - pixels as i64).abs())?;
    Some(Image {
        buffer: MemoryRenderBuffer::from_slice(
            &i.pixels_rgba,
            Fourcc::Argb8888,
            (i.width as i32, i.height as i32),
            1,
            Transform::Normal,
            None,
        ),
        hotspot: (i.xhot as i32, i.yhot as i32).into(),
        pixels: (i.width as i32, i.height as i32).into(),
        // The theme's sizes are fixed, so this is as near 1:1 as it gets.
        logical: (
            (i.width as f64 / scale).round() as i32,
            (i.height as f64 / scale).round() as i32,
        )
            .into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shape_draws_with_its_hotspot_inside() {
        for shape in Shape::ALL {
            let p = draw(shape, 29).unwrap_or_else(|| panic!("{shape:?} didn't draw"));
            assert!(p.bgra.chunks(4).any(|px| px[3] > 0), "{shape:?} is empty");
            let (x, y) = p.hotspot;
            assert!((0..29).contains(&x) && (0..29).contains(&y), "{shape:?}");
        }
    }

    #[test]
    fn names_map_to_shapes() {
        assert_eq!(Shape::of(CursorIcon::Default), Shape::Arrow);
        assert_eq!(Shape::of(CursorIcon::Pointer), Shape::Hand);
        assert_eq!(Shape::of(CursorIcon::NwseResize), Shape::ResizeNwse);
        assert_eq!(Shape::of(CursorIcon::SeResize), Shape::ResizeNwse);
        assert_eq!(Shape::of(CursorIcon::Grabbing), Shape::Move);
    }

    /// Draws every shape large, side by side on light and dark, into the
    /// PNG PATINA_CURSOR_PREVIEW names, to look at.
    #[test]
    #[ignore]
    fn preview() {
        let path = std::env::var("PATINA_CURSOR_PREVIEW").unwrap();
        let (side, n) = (96u32, Shape::ALL.len() as u32);
        let mut sheet = resvg::tiny_skia::Pixmap::new(side * n, side * 2).unwrap();
        sheet.fill_rect(
            resvg::tiny_skia::Rect::from_xywh(0.0, side as f32, (side * n) as f32, side as f32)
                .unwrap(),
            &resvg::tiny_skia::Paint {
                shader: resvg::tiny_skia::Shader::SolidColor(resvg::tiny_skia::Color::from_rgba8(
                    40, 42, 46, 255,
                )),
                ..Default::default()
            },
            resvg::tiny_skia::Transform::identity(),
            None,
        );
        let light =
            resvg::tiny_skia::Rect::from_xywh(0.0, 0.0, (side * n) as f32, side as f32).unwrap();
        let mut paint = resvg::tiny_skia::Paint::default();
        paint.set_color_rgba8(236, 236, 232, 255);
        sheet.fill_rect(light, &paint, resvg::tiny_skia::Transform::identity(), None);
        for (i, shape) in Shape::ALL.into_iter().enumerate() {
            let (body, _) = shape.drawing();
            let tree =
                resvg::usvg::Tree::from_str(&svg(&body), &resvg::usvg::Options::default()).unwrap();
            for row in 0..2 {
                let k = side as f32 / 32.0;
                let t = resvg::tiny_skia::Transform::from_scale(k, k)
                    .post_translate((i as u32 * side) as f32, (row * side) as f32);
                resvg::render(&tree, t, &mut sheet.as_mut());
            }
        }
        sheet.save_png(path).unwrap();
    }
}
