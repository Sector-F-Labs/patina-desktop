//! Weather icons painted from shapes, so they scale to any size and take
//! their colours from the palette (and so from the skin) rather than from a
//! font's emoji.

use egui::epaint::{Mesh, Vertex, WHITE_UV};
use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, Vec2, pos2, vec2};

use crate::forecast::{Sky, is_shower, sky};
use crate::palette::Palette;

/// Allocates a square and paints the icon for a weather code in it.
pub fn icon(ui: &mut egui::Ui, size: f32, code: u8, is_day: bool, palette: &Palette) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        paint(ui.painter(), rect, code, is_day, palette);
    }
    resp
}

pub fn paint(painter: &Painter, rect: Rect, code: u8, is_day: bool, p: &Palette) {
    let s = rect.width().min(rect.height());
    let c = rect.center();
    let light = |painter: &Painter, at: Pos2, r: f32| {
        if is_day {
            sun(painter, at, r, p.sun);
        } else {
            moon(painter, at, r, p.moon);
        }
    };
    match sky(code) {
        Sky::Clear if code == 1 => {
            light(painter, c + vec2(-0.05, -0.05) * s, 0.24 * s);
            cloud(painter, c + vec2(0.16, 0.2) * s, 0.42 * s, p.cloud);
        }
        Sky::Clear => light(painter, c, 0.26 * s),
        Sky::PartlyCloudy => {
            light(painter, c + vec2(-0.14, -0.14) * s, 0.21 * s);
            cloud(painter, c + vec2(0.06, 0.1) * s, 0.78 * s, p.cloud);
        }
        Sky::Cloudy => {
            cloud(painter, c + vec2(0.14, -0.1) * s, 0.55 * s, p.cloud.gamma_multiply(0.6));
            cloud(painter, c + vec2(-0.04, 0.08) * s, 0.8 * s, p.cloud);
        }
        Sky::Fog => {
            cloud(painter, c + vec2(0.0, -0.12) * s, 0.72 * s, p.cloud);
            for (i, w) in [0.62, 0.48].into_iter().enumerate() {
                let y = c.y + (0.2 + 0.12 * i as f32) * s;
                let stroke = Stroke::new(0.06 * s, p.cloud);
                painter.line_segment([pos2(c.x - w * s / 2.0, y), pos2(c.x + w * s / 2.0, y)], stroke);
            }
        }
        wet => {
            if is_shower(code) {
                light(painter, c + vec2(-0.18, -0.2) * s, 0.17 * s);
            }
            let top = c + vec2(0.02, -0.1) * s;
            let (drops, color) = match wet {
                Sky::Drizzle => (2, p.rain),
                Sky::Snow => (3, p.snow),
                Sky::Thunder => (2, p.rain),
                _ => (3, p.rain),
            };
            for i in 0..drops {
                let x = top.x + (i as f32 - (drops - 1) as f32 / 2.0) * 0.2 * s;
                let y = top.y + 0.32 * s;
                let at = pos2(x, y + if i % 2 == 1 { 0.08 * s } else { 0.0 });
                match wet {
                    Sky::Snow => flake(painter, at + vec2(0.0, 0.04 * s), 0.07 * s, color),
                    Sky::Sleet if i % 2 == 1 => flake(painter, at + vec2(0.0, 0.04 * s), 0.07 * s, p.snow),
                    _ => {
                        let len = if wet == Sky::Drizzle { 0.08 } else { 0.15 };
                        painter.line_segment([at, at + vec2(-0.05, len) * s], Stroke::new(0.055 * s, color));
                    }
                }
            }
            cloud(painter, top, 0.78 * s, p.cloud);
            if wet == Sky::Thunder {
                bolt(painter, top + vec2(0.08, 0.2) * s, 0.36 * s, p.lightning);
            }
        }
    }
}

fn sun(painter: &Painter, c: Pos2, r: f32, color: Color32) {
    painter.circle_filled(c, r * 0.62, color);
    for i in 0..8 {
        let a = i as f32 * std::f32::consts::TAU / 8.0;
        let d = Vec2::angled(a);
        painter.line_segment([c + d * r * 0.82, c + d * r * 1.08], Stroke::new(r * 0.16, color));
    }
}

/// A crescent: the part of one disc outside another, built as a triangle
/// strip because egui fills only convex paths.
fn moon(painter: &Painter, c: Pos2, r: f32, color: Color32) {
    let bite = c + vec2(0.45, -0.3) * r;
    let rb = 0.82 * r;
    let mut mesh = Mesh::default();
    let steps = 24;
    for k in 0..=steps {
        let y = -r + 2.0 * r * k as f32 / steps as f32;
        let half = (r * r - y * y).max(0.0).sqrt();
        let left = c.x - half;
        let right = c.x + half;
        let dy = c.y + y - bite.y;
        let inner = if dy.abs() < rb { bite.x - (rb * rb - dy * dy).sqrt() } else { right };
        let inner = inner.clamp(left, right);
        mesh.vertices.push(Vertex { pos: pos2(left, c.y + y), uv: WHITE_UV, color });
        mesh.vertices.push(Vertex { pos: pos2(inner, c.y + y), uv: WHITE_UV, color });
        if k > 0 {
            let i = (2 * k) as u32;
            mesh.add_triangle(i - 2, i - 1, i);
            mesh.add_triangle(i - 1, i + 1, i);
        }
    }
    painter.add(Shape::mesh(mesh));
}

/// A cloud `w` wide, centred on `c`.
fn cloud(painter: &Painter, c: Pos2, w: f32, color: Color32) {
    painter.circle_filled(c + vec2(-0.22, 0.04) * w, 0.19 * w, color);
    painter.circle_filled(c + vec2(0.0, -0.06) * w, 0.27 * w, color);
    painter.circle_filled(c + vec2(0.24, 0.05) * w, 0.18 * w, color);
    let base = Rect::from_min_max(c + vec2(-0.4, 0.04) * w, c + vec2(0.42, 0.23) * w);
    painter.rect_filled(base, 0.095 * w, color);
}

fn flake(painter: &Painter, c: Pos2, r: f32, color: Color32) {
    for i in 0..3 {
        let d = Vec2::angled(i as f32 * std::f32::consts::PI / 3.0 + 0.3) * r;
        painter.line_segment([c - d, c + d], Stroke::new(r * 0.38, color));
    }
}

fn bolt(painter: &Painter, top: Pos2, h: f32, color: Color32) {
    let pts = |v: &[(f32, f32)]| v.iter().map(|(x, y)| top + vec2(*x, *y) * h).collect::<Vec<_>>();
    // Two convex halves.
    painter.add(Shape::convex_polygon(
        pts(&[(0.1, 0.0), (0.35, 0.0), (0.18, 0.42), (-0.12, 0.5)]),
        color,
        Stroke::NONE,
    ));
    painter.add(Shape::convex_polygon(pts(&[(-0.02, 0.38), (0.3, 0.38), (-0.05, 1.0)]), color, Stroke::NONE));
}
