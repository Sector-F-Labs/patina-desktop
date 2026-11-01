//! Small icons the shell pieces draw themselves, from shapes in a colour
//! the caller takes from the skin, so they follow it like text does.

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, pos2, vec2};

/// A speaker in `rect`: with up to three waves for `level` (0 to 1), or a
/// cross when `muted`.
pub fn speaker(painter: &Painter, rect: Rect, level: f32, muted: bool, colour: Color32) {
    let s = rect.height().min(rect.width());
    let r = Rect::from_center_size(rect.center(), vec2(s, s));
    let at = |x: f32, y: f32| pos2(r.left() + x * s, r.top() + y * s);
    // The speaker: a box and its cone.
    painter.add(Shape::convex_polygon(
        vec![
            at(0.10, 0.38),
            at(0.28, 0.38),
            at(0.50, 0.18),
            at(0.50, 0.82),
            at(0.28, 0.62),
            at(0.10, 0.62),
        ],
        colour,
        Stroke::NONE,
    ));
    let stroke = Stroke::new((s * 0.075).max(1.0), colour);
    if muted {
        painter.line_segment([at(0.62, 0.38), at(0.86, 0.62)], stroke);
        painter.line_segment([at(0.62, 0.62), at(0.86, 0.38)], stroke);
        return;
    }
    let waves = if level <= 0.0 {
        0
    } else if level < 0.34 {
        1
    } else if level < 0.67 {
        2
    } else {
        3
    };
    let centre = at(0.50, 0.50);
    for i in 0..waves {
        let radius = s * (0.16 + 0.13 * i as f32);
        painter.add(Shape::line(arc(centre, radius), stroke));
    }
}

/// A sun in `rect` for the screen's brightness: a disc, and eight rays that
/// grow with `level` (0 to 1), as the speaker's waves do with the volume.
pub fn sun(painter: &Painter, rect: Rect, level: f32, colour: Color32) {
    let s = rect.height().min(rect.width());
    let centre = rect.center();
    painter.circle_filled(centre, s * 0.17, colour);
    let stroke = Stroke::new((s * 0.075).max(1.0), colour);
    let inner = s * 0.28;
    let outer = inner + s * (0.06 + 0.14 * level.clamp(0.0, 1.0));
    for i in 0..8 {
        let a = std::f32::consts::FRAC_PI_4 * i as f32;
        let d = vec2(a.cos(), a.sin());
        painter.line_segment([centre + d * inner, centre + d * outer], stroke);
    }
}

/// Points along a quarter-circle arc opening to the right.
fn arc(centre: Pos2, radius: f32) -> Vec<Pos2> {
    (0..=8)
        .map(|i| {
            let a = -std::f32::consts::FRAC_PI_4 + std::f32::consts::FRAC_PI_2 * i as f32 / 8.0;
            centre + vec2(a.cos(), a.sin()) * radius
        })
        .collect()
}

/// The power symbol in `rect`: a ring open at the top, and a bar down
/// through the gap, for the bar's power menu (ADR-0048).
pub fn power(painter: &Painter, rect: Rect, colour: Color32) {
    let s = rect.height().min(rect.width());
    let r = Rect::from_center_size(rect.center(), vec2(s, s));
    let centre = pos2(r.center().x, r.top() + 0.55 * s);
    let radius = s * 0.36;
    let stroke = Stroke::new((s * 0.09).max(1.0), colour);
    // Clockwise from the top right round to the top left, leaving a gap
    // either side of straight up (-90 degrees, with y down).
    let (from, to) = (-55f32.to_radians(), 235f32.to_radians());
    let ring: Vec<Pos2> = (0..=24)
        .map(|i| {
            let a = from + (to - from) * i as f32 / 24.0;
            centre + vec2(a.cos(), a.sin()) * radius
        })
        .collect();
    painter.add(Shape::line(ring, stroke));
    painter.line_segment(
        [pos2(centre.x, r.top() + 0.08 * s), pos2(centre.x, centre.y)],
        stroke,
    );
}

/// A Wi-Fi fan in `rect`: a dot and three arcs, lit up to `strength`
/// (0 to 100), the rest dimmed; with a slash when not connected (`None`).
pub fn wifi(painter: &Painter, rect: Rect, strength: Option<u8>, colour: Color32) {
    let s = rect.height().min(rect.width());
    let r = Rect::from_center_size(rect.center(), vec2(s, s));
    let base = pos2(r.center().x, r.top() + 0.82 * s);
    let dim = colour.gamma_multiply(0.3);
    let lit = match strength {
        None => 0,
        Some(v) if v >= 67 => 3,
        Some(v) if v >= 34 => 2,
        Some(v) if v > 0 => 1,
        Some(_) => 0,
    };
    let stroke_width = (s * 0.09).max(1.0);
    let dot_colour = if strength.is_some() { colour } else { dim };
    painter.circle_filled(base, stroke_width * 0.9, dot_colour);
    for i in 0..3 {
        let radius = s * (0.26 + 0.2 * i as f32);
        let c = if i < lit { colour } else { dim };
        let points: Vec<Pos2> = (0..=10)
            .map(|k| {
                // From up-left to up-right, the fan's opening.
                let a = -std::f32::consts::FRAC_PI_4 * 3.0
                    + std::f32::consts::FRAC_PI_2 * k as f32 / 10.0;
                base + vec2(a.cos(), a.sin()) * radius
            })
            .collect();
        painter.add(Shape::line(points, Stroke::new(stroke_width, c)));
    }
    if strength.is_none() {
        painter.line_segment(
            [
                pos2(r.left() + 0.18 * s, r.top() + 0.18 * s),
                pos2(r.right() - 0.18 * s, r.bottom() - 0.12 * s),
            ],
            Stroke::new(stroke_width, colour),
        );
    }
}

/// A battery lying on its side in `rect`, filled to `percent`, with a bolt
/// while `charging`. The bolt is drawn in `background` so it shows on the
/// fill.
pub fn battery(
    painter: &Painter,
    rect: Rect,
    percent: u8,
    charging: bool,
    colour: Color32,
    background: Color32,
) {
    let h = rect.height().min(rect.width() * 0.6);
    let w = h * 1.8;
    let body = Rect::from_center_size(rect.center() - vec2(h * 0.08, 0.0), vec2(w, h));
    let stroke = (h * 0.1).max(1.0);
    painter.rect_stroke(
        body,
        h * 0.18,
        Stroke::new(stroke, colour),
        egui::StrokeKind::Inside,
    );
    // The terminal nub.
    let nub = Rect::from_min_size(
        pos2(body.right(), body.center().y - h * 0.18),
        vec2(h * 0.14, h * 0.36),
    );
    painter.rect_filled(nub, h * 0.05, colour);
    let inner = body.shrink(stroke * 1.8);
    let fill = Rect::from_min_size(
        inner.min,
        vec2(
            inner.width() * percent.min(100) as f32 / 100.0,
            inner.height(),
        ),
    );
    painter.rect_filled(fill, h * 0.08, colour);
    if charging {
        let c = inner.center();
        let s = inner.height();
        let bolt = vec![
            c + vec2(0.10 * s, -0.55 * s),
            c + vec2(-0.30 * s, 0.08 * s),
            c + vec2(-0.02 * s, 0.08 * s),
            c + vec2(-0.10 * s, 0.55 * s),
            c + vec2(0.30 * s, -0.08 * s),
            c + vec2(0.02 * s, -0.08 * s),
        ];
        painter.add(Shape::Path(egui::epaint::PathShape {
            points: bolt,
            closed: true,
            fill: background,
            stroke: Stroke::new(stroke * 0.6, colour).into(),
        }));
    }
}

/// A play triangle in `rect`, filled.
pub fn play(painter: &Painter, rect: Rect, colour: Color32) {
    let s = rect.height().min(rect.width());
    let c = rect.center();
    painter.add(Shape::convex_polygon(
        vec![
            pos2(c.x - 0.3 * s, c.y - 0.42 * s),
            pos2(c.x - 0.3 * s, c.y + 0.42 * s),
            pos2(c.x + 0.42 * s, c.y),
        ],
        colour,
        Stroke::NONE,
    ));
}

/// Two bars in `rect`, filled.
pub fn pause(painter: &Painter, rect: Rect, colour: Color32) {
    let s = rect.height().min(rect.width());
    let c = rect.center();
    for dx in [-0.22, 0.22] {
        let bar = Rect::from_center_size(pos2(c.x + dx * s, c.y), vec2(0.24 * s, 0.76 * s));
        painter.rect_filled(bar, 0.0, colour);
    }
}

/// A triangle against a bar in `rect`, pointing right for the next track
/// or left for the one before.
pub fn skip(painter: &Painter, rect: Rect, forward: bool, colour: Color32) {
    let s = rect.height().min(rect.width());
    let c = rect.center();
    let d = if forward { 1.0 } else { -1.0 };
    painter.add(Shape::convex_polygon(
        vec![
            pos2(c.x - d * 0.34 * s, c.y - 0.36 * s),
            pos2(c.x - d * 0.34 * s, c.y + 0.36 * s),
            pos2(c.x + d * 0.22 * s, c.y),
        ],
        colour,
        Stroke::NONE,
    ));
    let bar = Rect::from_center_size(pos2(c.x + d * 0.34 * s, c.y), vec2(0.12 * s, 0.72 * s));
    painter.rect_filled(bar, 0.0, colour);
}
