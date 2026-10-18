//! Small pieces the views share: cards, the temperature range bar, and the
//! gauges on the overview's tiles. Colours come from the visuals or the
//! palette.

use egui::epaint::{Mesh, Vertex, WHITE_UV};
use egui::{Color32, Pos2, Rect, RichText, Stroke, Ui, Vec2, pos2, vec2};

use crate::palette::Palette;

/// Everything a view needs to draw a place's forecast.
pub struct Scene<'a> {
    pub forecast: &'a crate::forecast::Forecast,
    pub place: &'a crate::forecast::Place,
    pub units: crate::units::Units,
    pub palette: &'a Palette,
    /// Days the forecast list and the week show.
    pub days: usize,
}

/// Draws one card's contents, for the lists of cards the views lay out.
pub type Body<'a> = Box<dyn Fn(&mut Ui) + 'a>;

/// What a view asks the app to do.
pub enum Action {
    ShowDay(chrono::NaiveDate),
}

/// A rounded panel on the view's background, holding one subject.
pub fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> egui::InnerResponse<R> {
    let v = ui.visuals();
    egui::Frame::new()
        .fill(v.faint_bg_color)
        .stroke(v.widgets.noninteractive.bg_stroke)
        .corner_radius(12)
        .inner_margin(egui::Margin::symmetric(14, 12))
        .show(ui, add)
}

/// A card's heading: small, weak capitals, as on macOS Weather's tiles.
pub fn heading(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text.to_uppercase()).small().strong().color(ui.visuals().weak_text_color()));
}

/// A horizontal strip blending along the temperature scale from `from` to
/// `to` °C, with round ends.
pub fn temp_gradient(painter: &egui::Painter, rect: Rect, from: f32, to: f32, palette: &Palette) {
    let r = rect.height() / 2.0;
    let (left, right) = (rect.left() + r, (rect.right() - r).max(rect.left() + r));
    painter.circle_filled(pos2(left, rect.center().y), r, palette.temp(from));
    painter.circle_filled(pos2(right, rect.center().y), r, palette.temp(to));
    let steps = 16;
    let mut mesh = Mesh::default();
    for k in 0..=steps {
        let t = k as f32 / steps as f32;
        let x = left + (right - left) * t;
        let color = palette.temp(from + (to - from) * t);
        mesh.vertices.push(Vertex { pos: pos2(x, rect.top()), uv: WHITE_UV, color });
        mesh.vertices.push(Vertex { pos: pos2(x, rect.bottom()), uv: WHITE_UV, color });
        if k > 0 {
            let i = (2 * k) as u32;
            mesh.add_triangle(i - 2, i - 1, i);
            mesh.add_triangle(i - 1, i + 1, i);
        }
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// macOS Weather's range bar: a track for the lowest to highest temperature
/// of the days shown, coloured from this day's low to its high, with a dot
/// for now on today's.
pub fn range_bar(ui: &mut Ui, width: f32, span: (f32, f32), low: f32, high: f32, now: Option<f32>, palette: &Palette) {
    let (rect, _) = ui.allocate_exact_size(vec2(width, 6.0), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 3.0, ui.visuals().extreme_bg_color);
    let total = (span.1 - span.0).max(1.0);
    let x = |t: f32| rect.left() + rect.width() * ((t - span.0) / total).clamp(0.0, 1.0);
    let bar = Rect::from_min_max(pos2(x(low), rect.top()), pos2(x(high).max(x(low) + 6.0), rect.bottom()));
    temp_gradient(painter, bar, low, high, palette);
    if let Some(now) = now {
        let c = pos2(x(now), rect.center().y);
        painter.circle(c, 4.5, Color32::WHITE, Stroke::new(2.0, ui.visuals().faint_bg_color));
    }
}

/// A bar for a scale such as UV or air quality, with a dot at `t` (0..1).
pub fn scale_bar(ui: &mut Ui, width: f32, colors: &[Color32], t: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(width, 6.0), egui::Sense::hover());
    let painter = ui.painter();
    let n = colors.len().max(1);
    let r = 3.0;
    let mut mesh = Mesh::default();
    for (k, c) in colors.iter().enumerate() {
        let x = rect.left() + r + (rect.width() - 2.0 * r) * k as f32 / (n - 1).max(1) as f32;
        mesh.vertices.push(Vertex { pos: pos2(x, rect.top()), uv: WHITE_UV, color: *c });
        mesh.vertices.push(Vertex { pos: pos2(x, rect.bottom()), uv: WHITE_UV, color: *c });
        if k > 0 {
            let i = (2 * k) as u32;
            mesh.add_triangle(i - 2, i - 1, i);
            mesh.add_triangle(i - 1, i + 1, i);
        }
    }
    painter.circle_filled(pos2(rect.left() + r, rect.center().y), r, colors[0]);
    painter.circle_filled(pos2(rect.right() - r, rect.center().y), r, colors[n - 1]);
    painter.add(egui::Shape::mesh(mesh));
    let c = pos2(rect.left() + r + (rect.width() - 2.0 * r) * t.clamp(0.0, 1.0), rect.center().y);
    painter.circle(c, 4.5, Color32::WHITE, Stroke::new(2.0, ui.visuals().faint_bg_color));
}

/// A compass rose with an arrow showing where the wind blows to.
pub fn compass(ui: &mut Ui, size: f32, from_degrees: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
    let painter = ui.painter();
    let c = rect.center();
    let r = size / 2.0 - 2.0;
    let weak = ui.visuals().weak_text_color();
    painter.circle_stroke(c, r, Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color));
    for i in 0..36 {
        let d = Vec2::angled((i as f32 * 10.0).to_radians());
        let inner = if i % 9 == 0 { r - 7.0 } else { r - 3.0 };
        painter.line_segment([c + d * inner, c + d * r], Stroke::new(1.0, weak.gamma_multiply(0.6)));
    }
    let font = egui::FontId::proportional(10.0);
    for (label, deg) in [("N", 0.0f32), ("E", 90.0), ("S", 180.0), ("W", 270.0)] {
        let at = c + bearing(deg) * (r - 15.0);
        painter.text(at, egui::Align2::CENTER_CENTER, label, font.clone(), weak);
    }
    // The wind comes from `from_degrees`, so the arrow points the other way.
    let to = bearing(from_degrees + 180.0);
    let tail = c - to * (r - 4.0);
    let tip = c + to * (r - 4.0);
    painter.line_segment([tail, tip], Stroke::new(2.5, color));
    painter.circle_filled(tail, 3.5, color);
    arrow_head(painter, tip, to, 9.0, color);
}

/// The screen direction of a compass bearing: 0° up, 90° right.
pub fn bearing(degrees: f32) -> Vec2 {
    let a = degrees.to_radians();
    vec2(a.sin(), -a.cos())
}

pub fn arrow_head(painter: &egui::Painter, tip: Pos2, dir: Vec2, size: f32, color: Color32) {
    let back = tip - dir * size;
    let side = dir.rot90() * size * 0.55;
    painter.add(egui::Shape::convex_polygon(vec![tip, back + side, back - side], color, Stroke::NONE));
}

/// Half a dial from `min` to `max` with a needle at `value`, for pressure.
pub fn dial(ui: &mut Ui, size: f32, t: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(size, size * 0.62), egui::Sense::hover());
    let painter = ui.painter();
    let c = pos2(rect.center().x, rect.bottom() - 4.0);
    let r = size / 2.0 - 4.0;
    let weak = ui.visuals().weak_text_color();
    let n = 40;
    for i in 0..=n {
        let a = std::f32::consts::PI * (1.0 + i as f32 / n as f32);
        let d = Vec2::angled(a);
        let len = if i % 10 == 0 { 8.0 } else { 4.0 };
        painter.line_segment([c + d * (r - len), c + d * r], Stroke::new(1.0, weak.gamma_multiply(0.6)));
    }
    let a = std::f32::consts::PI * (1.0 + t.clamp(0.0, 1.0));
    let d = Vec2::angled(a);
    painter.line_segment([c + d * (r * 0.25), c + d * (r - 2.0)], Stroke::new(3.0, color));
    painter.circle_filled(c, 3.0, color);
}

/// The sun's path through the day: an arc from sunrise to sunset, with a
/// dot where the sun is now (or below the line at night).
pub fn sun_arc(ui: &mut Ui, width: f32, t: f32, palette: &Palette) {
    let (rect, _) = ui.allocate_exact_size(vec2(width, width * 0.42), egui::Sense::hover());
    let painter = ui.painter();
    let horizon = rect.top() + rect.height() * 0.7;
    let weak = ui.visuals().weak_text_color();
    painter.line_segment([pos2(rect.left(), horizon), pos2(rect.right(), horizon)], Stroke::new(1.0, weak));
    // A sine across the full day: below the horizon at night.
    let point = |u: f32| {
        let x = rect.left() + rect.width() * u;
        let y = horizon - (u * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2).sin() * rect.height() * 0.55;
        pos2(x, y.min(rect.bottom()))
    };
    let pts: Vec<Pos2> = (0..=48).map(|k| point(k as f32 / 48.0)).collect();
    painter.add(egui::Shape::line(pts, Stroke::new(1.5, weak.gamma_multiply(0.7))));
    let at = point(t.clamp(0.0, 1.0));
    let color = if at.y < horizon { palette.sun } else { weak };
    painter.circle_filled(at, 5.0, color);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forecast;

    /// Draws each view of the sample forecast at a few widths, in light and
    /// dark, through egui itself: none may panic, whatever day it is now.
    #[test]
    fn every_view_draws() {
        let f = forecast::parse(include_str!("../testdata/stockholm.json"), None).unwrap();
        let place =
            forecast::Place { name: "Stockholm".into(), region: String::new(), latitude: 59.3, longitude: 18.1 };
        for dark in [false, true] {
            let palette = Palette::new(dark);
            for days in [7, 14] {
                let scene = Scene { forecast: &f, place: &place, units: Default::default(), palette: &palette, days };
                for width in [500.0, 900.0, 1400.0] {
                    let ctx = egui::Context::default();
                    ctx.set_visuals(if dark { egui::Visuals::dark() } else { egui::Visuals::light() });
                    for frame in 0..2 {
                        let input = egui::RawInput {
                            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(width, 3000.0))),
                            ..Default::default()
                        };
                        let _ = ctx.run_ui(input, |ui| {
                            crate::overview::show(ui, &scene);
                            crate::week::show(ui, &scene);
                            let day = f.days[(frame * 5) % f.days.len()].date;
                            crate::day::show(ui, &scene, day);
                        });
                    }
                }
            }
        }
    }
}
