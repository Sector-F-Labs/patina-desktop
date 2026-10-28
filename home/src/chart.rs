//! Graphs of numeric sensors over the past hours (ADR-0006), drawn with egui_plot as the weather app
//! draws its own: temperature coloured along its scale, a soft fill, and the highs and lows marked.
//! Small ones sit on dashboard cards, and a window shows a big one with a range to pick.

use std::sync::Arc;

use chrono::{DateTime, Local, TimeZone};
use egui_plot::{GridMark, HoverPosition, Line, Plot, PlotPoint, PlotPoints, Points, Text};

use crate::ha::Entity;
use crate::history::{Points as Series, RANGES, Store};
use crate::palette::Palette;

/// What the numbers measure, which picks the graph's colour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Temperature { fahrenheit: bool },
    Power,
    Humidity,
    Other,
}

/// A sensor with a number and a unit is worth a graph.
pub fn is_graphable(e: &Entity) -> bool {
    e.domain() == "sensor" && !e.is_unavailable() && e.state.parse::<f64>().is_ok() && unit(e).is_some()
}

fn unit(e: &Entity) -> Option<&str> {
    e.attributes.get("unit_of_measurement").and_then(|u| u.as_str()).filter(|u| !u.is_empty())
}

pub fn kind(e: &Entity) -> Kind {
    let class = e.attributes.get("device_class").and_then(|c| c.as_str()).unwrap_or("");
    match (class, unit(e).unwrap_or("")) {
        (_, "°F") => Kind::Temperature { fahrenheit: true },
        ("temperature", _) | (_, "°C") => Kind::Temperature { fahrenheit: false },
        ("power" | "apparent_power", _) | (_, "W" | "kW" | "VA") => Kind::Power,
        ("humidity", _) | (_, "%") if class != "battery" => Kind::Humidity,
        _ => Kind::Other,
    }
}

/// Everything a graph needs about one sensor.
pub struct Chart<'a> {
    pub entity: &'a Entity,
    /// `None` while the history is on its way.
    pub points: Option<&'a Series>,
    pub hours: u32,
    pub palette: &'a Palette,
}

fn now() -> f64 {
    chrono::Utc::now().timestamp_millis() as f64 / 1000.0
}

fn local(seconds: f64) -> DateTime<Local> {
    Local.timestamp_millis_opt((seconds * 1000.0) as i64).single().unwrap_or_default()
}

impl Chart<'_> {
    /// The history with the current value at its end, as `[hours from now, value]`.
    fn series(&self) -> Vec<[f64; 2]> {
        let now = now();
        let mut series: Vec<[f64; 2]> =
            self.points.map(|p| p.iter().map(|[t, v]| [(t - now) / 3600.0, *v]).collect()).unwrap_or_default();
        if let Ok(current) = self.entity.state.parse::<f64>() {
            series.push([0.0, current]);
        }
        series
    }

    fn color(&self) -> egui::Color32 {
        match kind(self.entity) {
            Kind::Power => self.palette.power,
            Kind::Humidity => self.palette.humidity,
            _ => self.palette.line,
        }
    }

    fn plot(&self, id: &str, height: f32, full: bool, series: &[[f64; 2]]) -> Plot<'static> {
        let hours = self.hours;
        let offset = Local::now().offset().local_minus_utc() as f64;
        let unit = unit(self.entity).unwrap_or("").to_string();
        let series: Arc<Vec<[f64; 2]>> = Arc::new(series.to_vec());
        let mut plot = Plot::new(format!("history-{}-{hours}-{id}", self.entity.id))
            .height(height)
            .allow_zoom(false)
            .allow_drag(false)
            .allow_scroll(false)
            .allow_boxed_zoom(false)
            .allow_double_click_reset(false)
            .show_background(false)
            .include_x(-(hours as f64))
            .include_x(0.0)
            .set_margin_fraction(egui::vec2(0.0, 0.12))
            .label_formatter(move |pos| {
                let x = match pos {
                    HoverPosition::NearDataPoint { position, .. } => position.x,
                    HoverPosition::Elsewhere { position } => position.x,
                };
                let nearest = series.iter().min_by(|a, b| (a[0] - x).abs().total_cmp(&(b[0] - x).abs()))?;
                let t = local(now() + nearest[0] * 3600.0);
                let when = t.format(if hours > 48 { "%a %H:%M" } else { "%H:%M" });
                Some(format!("{when}\n{} {unit}", trim(nearest[1])))
            });
        if full {
            // Hours along the bottom, on the clock: every hour or three, six, or a day.
            let (big, small) = match hours {
                0..=6 => (3.0, 1.0),
                7..=48 => (6.0, 3.0),
                _ => (24.0, 12.0),
            };
            let origin = now();
            plot = plot
                .show_grid([true, true])
                .y_axis_min_width(40.0)
                .x_grid_spacer(move |input| {
                    let (lo, hi) = input.bounds;
                    // Clock hours since local midnight at `lo`'s epoch: marks land on whole hours.
                    let clock = |x: f64| (origin + x * 3600.0 + offset) / 3600.0;
                    let mut marks = Vec::new();
                    let mut h = (clock(lo) / small).ceil() * small;
                    while (h - clock(0.0)) <= hi {
                        let x = h - clock(0.0);
                        let step = if (h / big).fract().abs() < 1e-6 { big } else { small };
                        marks.push(GridMark { value: x, step_size: step });
                        h += small;
                    }
                    marks
                })
                .x_axis_formatter(move |mark, _| {
                    let t = local(origin + mark.value * 3600.0);
                    if hours > 48 { t.format("%a %-d").to_string() } else { t.format("%H").to_string() }
                })
                .y_axis_formatter(|m, _| trim(m.value));
        } else {
            plot = plot.show_axes([false, false]).show_grid([false, false]).show_x(false).show_y(false);
        }
        plot
    }

    /// The small graph on a card. Returns whether it was clicked.
    pub fn inline(&self, ui: &mut egui::Ui, height: f32) -> bool {
        let Some(series) = self.drawable(ui, height) else { return false };
        let (floor, top) = bounds(&series);
        let response = self
            .plot("inline", height, false, &series)
            .include_y(floor)
            .include_y(top)
            .show(ui, |plot| self.lines(plot, &series, floor, false));
        response.response.clicked()
    }

    /// The big graph, with axes.
    pub fn full(&self, ui: &mut egui::Ui, height: f32) {
        let Some(series) = self.drawable(ui, height) else { return };
        let (floor, top) = bounds(&series);
        self.plot("full", height, true, &series).include_y(floor).include_y(top).show(ui, |plot| {
            self.lines(plot, &series, floor, true);
        });
    }

    /// The series, or a note in its place while there's nothing to draw.
    fn drawable(&self, ui: &mut egui::Ui, height: f32) -> Option<Vec<[f64; 2]>> {
        let series = self.series();
        let note = match self.points {
            None => Some("Loading history…"),
            Some(_) if series.len() < 2 => Some("No history"),
            _ => None,
        };
        if let Some(note) = note {
            ui.allocate_ui(egui::vec2(ui.available_width(), height), |ui| {
                ui.centered_and_justified(|ui| ui.weak(note));
            });
            return None;
        }
        Some(series)
    }

    fn lines(&self, plot: &mut egui_plot::PlotUi<'_>, series: &[[f64; 2]], floor: f64, marks: bool) {
        let text = plot.ctx().global_style().visuals.text_color();
        let line = Line::new(self.entity.name(), PlotPoints::from(series.to_vec()))
            .fill(floor as f32)
            .fill_alpha(0.18)
            .width(if marks { 2.5 } else { 2.0 });
        plot.line(match kind(self.entity) {
            Kind::Temperature { fahrenheit } => {
                let palette = self.palette.clone();
                let colour = move |pt: PlotPoint| {
                    let c = if fahrenheit { (pt.y as f32 - 32.0) * 5.0 / 9.0 } else { pt.y as f32 };
                    palette.temp(c)
                };
                line.gradient_color(Arc::new(colour), true)
            }
            _ => line.color(self.color()),
        });
        if marks {
            let high = series.iter().copied().fold([0.0, f64::NEG_INFINITY], |a, b| if b[1] > a[1] { b } else { a });
            let low = series.iter().copied().fold([0.0, f64::INFINITY], |a, b| if b[1] < a[1] { b } else { a });
            let pad = (high[1] - low[1]).max(1.0) * 0.06;
            for (pt, kind, anchor, off) in
                [(high, "H", egui::Align2::CENTER_BOTTOM, pad), (low, "L", egui::Align2::CENTER_TOP, -pad)]
            {
                plot.points(Points::new("", vec![pt]).radius(3.5).color(text).allow_hover(false));
                plot.text(
                    Text::new("", PlotPoint::new(pt[0], pt[1] + off), format!("{kind} {}", trim(pt[1])))
                        .anchor(anchor)
                        .color(text)
                        .allow_hover(false),
                );
            }
        }
    }
}

/// A little under the lowest value and a little over the highest, so the line doesn't touch the edges.
fn bounds(series: &[[f64; 2]]) -> (f64, f64) {
    let low = series.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
    let high = series.iter().map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max);
    let pad = ((high - low) * 0.15).max(0.5);
    (low - pad, high + pad)
}

/// 21.5, 412, 0.043: as few decimals as read well.
pub fn trim(v: f64) -> String {
    let a = v.abs();
    let s = if a >= 100.0 {
        format!("{v:.0}")
    } else if a >= 10.0 {
        format!("{v:.1}")
    } else {
        format!("{v:.2}")
    };
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s }
}

/// The big graph's window, opened by clicking a sensor's name.
pub struct SensorDialog {
    pub id: String,
    hours: u32,
}

impl SensorDialog {
    pub fn new(id: String) -> Self {
        Self { id, hours: 24 }
    }

    /// Draws the window and asks for its history in `wanted`; false once it's closed.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        e: &Entity,
        history: &Store,
        palette: &Palette,
        wanted: &mut Vec<(String, u32)>,
    ) -> bool {
        let mut open = true;
        egui::Window::new(e.name())
            .id(egui::Id::new("sensor-history"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for (hours, label) in RANGES {
                        ui.selectable_value(&mut self.hours, hours, label);
                    }
                });
                wanted.push((e.id.clone(), self.hours));
                let points = history.get(&e.id, self.hours);
                let chart = Chart { entity: e, points, hours: self.hours, palette };
                ui.set_width(560.0);
                ui.horizontal(|ui| {
                    ui.heading(e.display_state());
                    if let Some(p) = points.filter(|p| !p.is_empty()) {
                        let values = p.iter().map(|p| p[1]);
                        let (min, max) =
                            values.clone().fold((f64::INFINITY, f64::NEG_INFINITY), |a, v| (a.0.min(v), a.1.max(v)));
                        let mean = values.sum::<f64>() / p.len() as f64;
                        let u = unit(e).unwrap_or("");
                        ui.weak(format!("   min {} · avg {} · max {} {u}", trim(min), trim(mean), trim(max)));
                    }
                });
                chart.full(ui, 260.0);
            });
        open && !ctx.input(|i| i.key_pressed(egui::Key::Escape))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sensor(id: &str, state: &str, attributes: serde_json::Value) -> Entity {
        Entity { id: id.into(), state: state.into(), attributes: attributes.as_object().unwrap().clone() }
    }

    #[test]
    fn only_numeric_sensors_with_a_unit_are_graphed() {
        assert!(is_graphable(&sensor("sensor.t", "21.5", json!({ "unit_of_measurement": "°C" }))));
        assert!(!is_graphable(&sensor("sensor.t", "21.5", json!({}))));
        assert!(!is_graphable(&sensor("sensor.s", "sunny", json!({ "unit_of_measurement": "x" }))));
        assert!(!is_graphable(&sensor("light.l", "5", json!({ "unit_of_measurement": "x" }))));
        assert!(!is_graphable(&sensor("sensor.t", "unavailable", json!({ "unit_of_measurement": "°C" }))));
    }

    #[test]
    fn kinds_come_from_class_then_unit() {
        let k = |class: &str, unit: &str| {
            kind(&sensor("sensor.x", "1", json!({ "device_class": class, "unit_of_measurement": unit })))
        };
        assert_eq!(k("", "°F"), Kind::Temperature { fahrenheit: true });
        assert_eq!(k("temperature", "K"), Kind::Temperature { fahrenheit: false });
        assert_eq!(k("", "kW"), Kind::Power);
        assert_eq!(k("humidity", "%"), Kind::Humidity);
        assert_eq!(k("battery", "%"), Kind::Other);
        assert_eq!(k("", "lx"), Kind::Other);
    }

    #[test]
    fn numbers_get_as_few_decimals_as_read_well() {
        assert_eq!(
            (trim(412.0), trim(21.5), trim(0.043), trim(21.0)),
            ("412".into(), "21.5".into(), "0.04".into(), "21".into())
        );
    }
}
