//! The Performance view's graphs: readings over the last minute, kept as
//! [`Series`], drawn large with a scale and the value under the pointer,
//! or small as the sidebar's sparklines. Colours only from the visuals, as
//! Settings' battery graphs (ADR-0081), so skins reach them.

use std::collections::VecDeque;

use egui::{Align2, Color32, Mesh, Pos2, Rect, Sense, Shape, Stroke, Ui, pos2, vec2};

/// How long a series is kept, in seconds: more than a graph shows, so a
/// slower update speed still fills it.
const KEEP: f64 = 300.0;
/// The room left of a large graph for its scale.
const GUTTER: f32 = 64.0;

/// Readings over time: seconds since the process manager started, and the
/// value.
#[derive(Debug, Clone, Default)]
pub struct Series(VecDeque<(f64, f64)>);

impl Series {
    pub fn push(&mut self, at: f64, value: f64) {
        self.0.push_back((at, value));
        while self.0.front().is_some_and(|(t, _)| at - t > KEEP) {
            self.0.pop_front();
        }
    }

    pub fn last(&self) -> Option<f64> {
        self.0.back().map(|(_, v)| *v)
    }

    /// The most in the last `span` seconds up to `now`.
    fn most(&self, now: f64, span: f64) -> f64 {
        self.0
            .iter()
            .filter(|(t, _)| now - t <= span)
            .map(|(_, v)| *v)
            .fold(0.0, f64::max)
    }

    /// The reading nearest `at`.
    fn nearest(&self, at: f64) -> Option<(f64, f64)> {
        self.0
            .iter()
            .copied()
            .min_by(|a, b| (a.0 - at).abs().total_cmp(&(b.0 - at).abs()))
    }
}

/// What a graph's values are.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Scale {
    /// 0 to 100.
    Percent,
    /// Bytes per second, the top found from the most there was.
    Rate,
    /// Bytes, the same way.
    Bytes,
    /// A percentage that's often small, as one process's share of the
    /// machine: the top is 1, 2 or 5 of a power of ten above the most
    /// there was, up to 100.
    Share,
}

impl Scale {
    fn top(self, lines: &[Line], now: f64, span: f64) -> f64 {
        let most = || {
            lines
                .iter()
                .map(|l| l.series.most(now, span))
                .fold(0.0, f64::max)
        };
        match self {
            Scale::Percent => 100.0,
            Scale::Rate => rate_top(most()),
            Scale::Bytes => bytes_top(most()),
            Scale::Share => share_top(most()),
        }
    }

    pub fn format(self, value: f64) -> String {
        match self {
            Scale::Percent => format!("{value:.0}%"),
            Scale::Share if value < 10.0 && value.fract() != 0.0 => format!("{value:.1}%"),
            Scale::Share => format!("{value:.0}%"),
            Scale::Rate => rate(value),
            Scale::Bytes => bytes(value),
        }
    }
}

/// A line on a graph.
pub struct Line<'a> {
    pub series: &'a Series,
    pub colour: Color32,
    /// What it is, for the value under the pointer ("Read").
    pub name: &'a str,
}

/// The scale's top for bytes per second: 1, 2 or 5 of a unit above the
/// most there was, and no less than 64 KiB/s, so quiet isn't drawn as
/// busy.
pub fn rate_top(most: f64) -> f64 {
    let floor = 64.0 * 1024.0;
    let want = (most * 1.1).max(floor);
    let mut unit = 1024.0;
    loop {
        for step in [1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0] {
            if step * unit >= want {
                return step * unit;
            }
        }
        unit *= 1024.0;
    }
}

/// The scale's top for bytes: as for a rate, from 1 MiB.
pub fn bytes_top(most: f64) -> f64 {
    rate_top(most).max(1024.0 * 1024.0)
}

/// The scale's top for a small percentage: 1, 2, 5, 10, 20, 50 or 100.
pub fn share_top(most: f64) -> f64 {
    [1.0, 2.0, 5.0, 10.0, 20.0, 50.0]
        .into_iter()
        .find(|top| *top >= most * 1.1)
        .unwrap_or(100.0)
}

/// `bytes` with a binary unit: "512 B", "3.4 MiB", "12 GiB".
pub fn bytes(bytes: f64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 || value >= 100.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

pub fn rate(per_second: f64) -> String {
    format!("{}/s", bytes(per_second))
}

/// A large graph of `lines` over the `span` seconds up to `now`, `height`
/// tall, filling the width; the first line is shaded under.
pub fn graph(ui: &mut Ui, lines: &[Line], scale: Scale, now: f64, span: f64, height: f32) {
    let v = ui.visuals().clone();
    let font = egui::TextStyle::Small.resolve(ui.style());
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, height + 18.0), Sense::hover());
    let plot = Rect::from_min_max(
        rect.min + vec2(GUTTER, 0.0),
        pos2(rect.max.x, rect.min.y + height),
    );
    let top = scale.top(lines, now, span);
    let painter = ui.painter_at(rect);
    let inside = ui.painter_at(plot);
    let faint = v.widgets.noninteractive.bg_stroke;

    painter.rect_filled(plot, 0.0, v.extreme_bg_color);
    // A line at each quarter, labelled at the top and the middle; a line
    // every ten seconds.
    for quarter in 0..=4 {
        let value = top * f64::from(quarter) / 4.0;
        let at = plot.bottom() - quarter as f32 / 4.0 * plot.height();
        if quarter > 0 && quarter < 4 {
            inside.hline(plot.x_range(), at, faint);
        }
        if quarter == 2 || quarter == 4 {
            painter.text(
                pos2(plot.left() - 6.0, at),
                if quarter == 4 {
                    Align2::RIGHT_TOP
                } else {
                    Align2::RIGHT_CENTER
                },
                scale.format(value),
                font.clone(),
                v.weak_text_color(),
            );
        }
    }
    let x = |t: f64| plot.right() - ((now - t) / span) as f32 * plot.width();
    let mut tick = 10.0;
    while tick < span {
        inside.vline(x(now - tick), plot.y_range(), faint);
        tick += 10.0;
    }
    painter.text(
        plot.left_bottom() + vec2(0.0, 3.0),
        Align2::LEFT_TOP,
        format!("{span:.0} seconds"),
        font.clone(),
        v.weak_text_color(),
    );
    painter.text(
        plot.right_bottom() + vec2(0.0, 3.0),
        Align2::RIGHT_TOP,
        "now",
        font.clone(),
        v.weak_text_color(),
    );

    for (i, line) in lines.iter().enumerate() {
        draw(&inside, plot, line, top, now, span, i == 0);
    }
    painter.rect_stroke(plot, 0.0, faint, egui::StrokeKind::Inside);

    // The readings nearest the pointer.
    let hovered = response
        .hover_pos()
        .filter(|p| plot.x_range().contains(p.x));
    if let Some(pointer) = hovered {
        let at = now - f64::from((plot.right() - pointer.x) / plot.width()) * span;
        let mut text = Vec::new();
        let mut marked = None;
        for line in lines {
            if let Some((t, value)) = line.series.nearest(at) {
                let y = plot.bottom() - (value / top).clamp(0.0, 1.0) as f32 * plot.height();
                inside.circle_filled(pos2(x(t), y), 3.5, line.colour);
                marked.get_or_insert(x(t));
                let what = scale.format(value);
                text.push(if line.name.is_empty() {
                    what
                } else {
                    format!("{} {what}", line.name)
                });
            }
        }
        if let Some(mark) = marked {
            inside.vline(mark, plot.y_range(), Stroke::new(1.0, v.text_color()));
            let ago = (now - at).max(0.0);
            inside.text(
                plot.right_top() + vec2(-6.0, 4.0),
                Align2::RIGHT_TOP,
                format!("{ago:.0} s ago · {}", text.join(" · ")),
                font,
                v.strong_text_color(),
            );
        }
    }
}

/// A small graph of `lines` in `rect`, without a scale, as the sidebar
/// shows each resource.
pub fn sparkline(ui: &Ui, rect: Rect, lines: &[Line], scale: Scale, now: f64, span: f64) {
    let v = ui.visuals();
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, v.extreme_bg_color);
    let top = scale.top(lines, now, span);
    for (i, line) in lines.iter().enumerate() {
        draw(&painter, rect, line, top, now, span, i == 0);
    }
    painter.rect_stroke(
        rect,
        2.0,
        v.widgets.noninteractive.bg_stroke,
        egui::StrokeKind::Inside,
    );
}

fn draw(
    painter: &egui::Painter,
    plot: Rect,
    line: &Line,
    top: f64,
    now: f64,
    span: f64,
    shaded: bool,
) {
    // From the last reading before the span, so the line starts at its edge.
    let readings = &line.series.0;
    let first = readings
        .iter()
        .rposition(|(t, _)| now - t > span)
        .unwrap_or(0);
    let points: Vec<Pos2> = readings
        .iter()
        .skip(first)
        .map(|(t, value)| {
            pos2(
                plot.right() - ((now - t) / span) as f32 * plot.width(),
                plot.bottom() - (value / top).clamp(0.0, 1.0) as f32 * plot.height(),
            )
        })
        .collect();
    if points.len() < 2 {
        return;
    }
    if shaded {
        let fill = line.colour.gamma_multiply(0.25);
        let mut mesh = Mesh::default();
        for (i, p) in points.iter().enumerate() {
            mesh.colored_vertex(*p, fill);
            mesh.colored_vertex(pos2(p.x, plot.bottom()), fill);
            if i > 0 {
                let b = 2 * i as u32;
                mesh.add_triangle(b - 2, b - 1, b);
                mesh.add_triangle(b - 1, b + 1, b);
            }
        }
        painter.add(Shape::mesh(mesh));
    }
    painter.add(Shape::line(points, Stroke::new(1.5, line.colour)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_series_keeps_five_minutes() {
        let mut s = Series::default();
        for t in 0..400 {
            s.push(f64::from(t), f64::from(t));
        }
        assert_eq!(s.0.len(), 301);
        assert_eq!(s.last(), Some(399.0));
        assert_eq!(s.most(399.0, 60.0), 399.0);
        assert_eq!(s.nearest(150.4), Some((150.0, 150.0)));
    }

    #[test]
    fn the_rate_scale_rounds_up_to_one_two_or_five_of_a_unit() {
        assert_eq!(rate_top(0.0), 100.0 * 1024.0);
        assert_eq!(rate_top(150.0 * 1024.0), 200.0 * 1024.0);
        assert_eq!(rate_top(3.0 * 1024.0 * 1024.0), 5.0 * 1024.0 * 1024.0);
        assert_eq!(rate_top(600.0 * 1024.0 * 1024.0), 1024.0 * 1024.0 * 1024.0);
    }

    #[test]
    fn a_small_share_is_drawn_tall() {
        assert_eq!(share_top(0.0), 1.0);
        assert_eq!(share_top(3.0), 5.0);
        assert_eq!(share_top(48.0), 100.0);
        assert_eq!(Scale::Share.format(0.5), "0.5%");
    }

    #[test]
    fn bytes_read_in_binary_units() {
        assert_eq!(bytes(512.0), "512 B");
        assert_eq!(bytes(1536.0), "1.5 KiB");
        assert_eq!(bytes(300.0 * 1024.0 * 1024.0), "300 MiB");
        assert_eq!(rate(2.0 * 1024.0 * 1024.0 * 1024.0), "2.0 GiB/s");
    }
}
