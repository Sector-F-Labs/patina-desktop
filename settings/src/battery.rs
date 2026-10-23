//! The battery, under the power settings: how it stands, how worn it is,
//! and graphs of its charge and power draw over the last day or week, from
//! the history UPower keeps (`patina_system::battery_history`). Nothing on
//! a machine without a battery.

use std::sync::mpsc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chrono::{Local, TimeZone, Timelike};
use egui::{Align2, Mesh, Pos2, Rect, RichText, Sense, Shape, Stroke, Ui, pos2, vec2};
use patina_system::battery_history::{self, History, Sample};
use patina_system::power::{self, Battery, Charge, Health};

/// UPower records as things change; once a minute is plenty to follow it.
const POLL: Duration = Duration::from_secs(60);
/// The plot's height, and the room left of it for the scale.
const HEIGHT: f32 = 110.0;
const GUTTER: f32 = 40.0;
/// Wider than this, a day's graph is mostly stretched lines.
const WIDEST: f32 = 680.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Span {
    Day,
    Week,
}

impl Span {
    fn seconds(self) -> u64 {
        match self {
            Span::Day => 24 * 60 * 60,
            Span::Week => 7 * 24 * 60 * 60,
        }
    }

    /// How many points UPower averages the span into.
    fn points(self) -> u32 {
        match self {
            Span::Day => 240,
            Span::Week => 420,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Span::Day => "Last 24 hours",
            Span::Week => "Last 7 days",
        }
    }
}

/// What the thread last read.
#[derive(Debug, Clone, Default)]
pub struct Readings {
    pub battery: Option<Battery>,
    pub health: Option<Health>,
    /// The day's history, then the week's.
    pub history: [Option<Result<Option<History>, String>>; 2],
}

pub struct BatterySection {
    span: Span,
    readings: Readings,
    updates: mpsc::Receiver<Readings>,
}

impl BatterySection {
    pub fn new(ctx: &egui::Context) -> Self {
        Self {
            span: Span::Day,
            readings: Readings::default(),
            updates: watch(ctx.clone()),
        }
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        while let Ok(readings) = self.updates.try_recv() {
            self.readings = readings;
        }
        let now = now();
        show(ui, &self.readings, &mut self.span, now);
    }
}

/// The section, from `readings`, at `now` (seconds since the epoch).
pub fn show(ui: &mut Ui, readings: &Readings, span: &mut Span, now: u64) {
    let Some(battery) = readings.battery else {
        return;
    };
    ui.add_space(16.0);
    ui.label(RichText::new("Battery").strong());
    ui.label(battery.describe());
    if let Some(health) = readings.health {
        ui.label(RichText::new(health.describe()).small().weak());
    }
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        for s in [Span::Day, Span::Week] {
            ui.selectable_value(span, s, s.label());
        }
    });
    let index = match span {
        Span::Day => 0,
        Span::Week => 1,
    };
    match &readings.history[index] {
        None => {}
        Some(Err(e)) => {
            ui.label(RichText::new(format!("Can't read the battery's history: {e}")).weak());
        }
        Some(Ok(None)) => {
            ui.label(
                RichText::new(
                    "The battery's history comes from UPower, which isn't running here \
                     (apt install upower on Ubuntu).",
                )
                .weak(),
            );
        }
        Some(Ok(Some(history))) => {
            ui.add_space(4.0);
            ui.label("Charge");
            graph(ui, &history.charge, *span, now, Unit::Percent);
            ui.add_space(8.0);
            ui.label("Power draw");
            graph(ui, &history.rate, *span, now, Unit::Watts);
            let from = now.saturating_sub(span.seconds());
            if let Some(watts) = average_draw(&history.rate, from) {
                ui.add_space(4.0);
                ui.label(
                    RichText::new(format!(
                        "On battery it drew {watts:.1} W on average. Shaded: plugged in."
                    ))
                    .small()
                    .weak(),
                );
            }
        }
    }
}

/// Reads the battery once a minute, on a thread, sending each reading.
fn watch(ctx: egui::Context) -> mpsc::Receiver<Readings> {
    let (send, receive) = mpsc::channel();
    let _ = std::thread::Builder::new()
        .name("battery".into())
        .spawn(move || {
            loop {
                let battery = power::read();
                let readings = Readings {
                    battery,
                    health: battery.and_then(|_| power::health()),
                    // Not worth asking UPower without a battery.
                    history: if battery.is_some() {
                        [Span::Day, Span::Week]
                            .map(|s| Some(battery_history::read(s.seconds() as u32, s.points())))
                    } else {
                        [None, None]
                    },
                };
                if send.send(readings).is_err() {
                    return;
                }
                ctx.request_repaint();
                std::thread::sleep(POLL);
            }
        });
    receive
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Unit {
    Percent,
    Watts,
}

impl Unit {
    fn format(self, value: f64) -> String {
        match self {
            Unit::Percent => format!("{value:.0}%"),
            Unit::Watts if value < 10.0 && value.fract() != 0.0 => format!("{value:.1} W"),
            Unit::Watts => format!("{value:.0} W"),
        }
    }
}

/// The scale's top for watts: the most drawn with room above, in fives.
fn watts_top(samples: &[Sample]) -> f64 {
    let most = samples.iter().map(|s| s.value).fold(0.0, f64::max);
    ((most * 1.1 / 5.0).ceil() * 5.0).max(5.0)
}

/// The mean draw while on battery since `from`, where it drew any.
fn average_draw(rate: &[Sample], from: u64) -> Option<f64> {
    let on_battery: Vec<f64> = rate
        .iter()
        .filter(|s| s.time >= from && s.state == Some(Charge::Discharging) && s.value > 0.0)
        .map(|s| s.value)
        .collect();
    (!on_battery.is_empty()).then(|| on_battery.iter().sum::<f64>() / on_battery.len() as f64)
}

/// The stretches it was plugged in, each from a sample saying so to the
/// next saying otherwise (or `now`), as one.
fn plugged(samples: &[Sample], now: u64) -> Vec<(u64, u64)> {
    let mut stretches: Vec<(u64, u64)> = Vec::new();
    for (i, s) in samples.iter().enumerate() {
        if !s.plugged() {
            continue;
        }
        let until = samples.get(i + 1).map_or(now, |n| n.time);
        match stretches.last_mut() {
            Some(last) if last.1 == s.time => last.1 = until,
            _ => stretches.push((s.time, until)),
        }
    }
    stretches
}

/// Where the time axis is marked, and what with: every six hours across a
/// day (a line and its time), each day across a week (a line at midnight,
/// the day's name at noon).
fn ticks(from: u64, to: u64, span: Span) -> Vec<(u64, Option<u64>, String)> {
    let local = |t: u64| Local.timestamp_opt(t as i64, 0).single();
    let mut ticks = Vec::new();
    // The first whole hour after `from`, then hour by hour.
    let mut t = from - from % 3600 + 3600;
    while t <= to {
        if let Some(time) = local(t) {
            match span {
                Span::Day if time.hour() % 6 == 0 => {
                    ticks.push((t, Some(t), time.format("%H:%M").to_string()));
                }
                Span::Week if time.hour() == 0 => {
                    // Named at noon, where there's room for it.
                    let noon = t + 12 * 3600;
                    let named = (noon < to).then_some(noon);
                    let name = local(noon).map_or(String::new(), |n| n.format("%a").to_string());
                    ticks.push((t, named, name));
                }
                _ => {}
            }
        }
        t += 3600;
    }
    ticks
}

/// A graph of `samples` over the span up to `now`, shaded where it was
/// plugged in, with the value under the pointer.
fn graph(ui: &mut Ui, samples: &[Sample], span: Span, now: u64, unit: Unit) {
    let v = ui.visuals().clone();
    let font = egui::TextStyle::Small.resolve(ui.style());
    let width = ui.available_width().min(WIDEST);
    let (rect, response) = ui.allocate_exact_size(vec2(width, HEIGHT + 18.0), Sense::hover());
    let plot = Rect::from_min_max(
        rect.min + vec2(GUTTER, 0.0),
        pos2(rect.max.x, rect.min.y + HEIGHT),
    );
    let from = now.saturating_sub(span.seconds());
    let top = match unit {
        Unit::Percent => 100.0,
        Unit::Watts => watts_top(samples),
    };
    let x = |t: u64| {
        plot.left() + (t as f32 - from as f32) / span.seconds() as f32 * plot.width()
    };
    let y = |value: f64| plot.bottom() - (value / top).clamp(0.0, 1.0) as f32 * plot.height();
    let painter = ui.painter_at(rect);
    let inside = ui.painter_at(plot);
    let faint = v.widgets.noninteractive.bg_stroke;
    let accent = v.selection.bg_fill;

    painter.rect_filled(plot, 0.0, v.extreme_bg_color);
    // Plugged in, a band for each stretch of it.
    let shade = v.text_color().gamma_multiply(0.07);
    for (start, end) in plugged(samples, now) {
        if end > from {
            inside.rect_filled(
                Rect::from_x_y_ranges(x(start.max(from))..=x(end), plot.y_range()),
                0.0,
                shade,
            );
        }
    }
    // The scale: a line at each quarter, labelled at the top and middle.
    for quarter in 0..=4 {
        let value = top * f64::from(quarter) / 4.0;
        let at = y(value);
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
                unit.format(value),
                font.clone(),
                v.weak_text_color(),
            );
        }
    }
    for (line, named, label) in ticks(from, now, span) {
        inside.vline(x(line), plot.y_range(), faint);
        if let Some(at) = named {
            painter.text(
                pos2(x(at), plot.bottom() + 3.0),
                Align2::CENTER_TOP,
                label,
                font.clone(),
                v.weak_text_color(),
            );
        }
    }

    // From the last sample before the span, so the line starts at its edge.
    let first = samples.iter().rposition(|s| s.time < from).unwrap_or(0);
    let points: Vec<Pos2> = samples[first..]
        .iter()
        .map(|s| pos2(x(s.time), y(s.value)))
        .collect();
    if points.len() >= 2 {
        let fill = accent.gamma_multiply(0.25);
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
        inside.add(Shape::mesh(mesh));
        inside.add(Shape::line(points, Stroke::new(1.5, accent)));
    } else {
        painter.text(
            plot.center(),
            Align2::CENTER_CENTER,
            "Nothing recorded yet",
            font.clone(),
            v.weak_text_color(),
        );
    }
    painter.rect_stroke(plot, 0.0, faint, egui::StrokeKind::Inside);

    // The sample nearest the pointer, and what it was.
    let hovered = response.hover_pos().filter(|p| plot.x_range().contains(p.x));
    if let Some(pointer) = hovered {
        let at = from as f32 + (pointer.x - plot.left()) / plot.width() * span.seconds() as f32;
        let nearest = samples
            .iter()
            .filter(|s| s.time >= from)
            .min_by_key(|s| (s.time as f32 - at).abs() as u64);
        if let Some(s) = nearest {
            let point = pos2(x(s.time), y(s.value));
            inside.vline(point.x, plot.y_range(), Stroke::new(1.0, v.text_color()));
            inside.circle_filled(point, 3.5, accent);
            let when = Local
                .timestamp_opt(s.time as i64, 0)
                .single()
                .map_or(String::new(), |t| t.format("%a %H:%M").to_string());
            let state = match s.state {
                Some(Charge::Charging) => " · charging",
                Some(Charge::Full) => " · full",
                Some(Charge::NotCharging) => " · plugged in",
                Some(Charge::Discharging) => " · on battery",
                None => "",
            };
            inside.text(
                plot.right_top() + vec2(-6.0, 4.0),
                Align2::RIGHT_TOP,
                format!("{when} · {}{state}", unit.format(s.value)),
                font,
                v.strong_text_color(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(time: u64, value: f64, state: Charge) -> Sample {
        Sample {
            time,
            value,
            state: Some(state),
        }
    }

    #[test]
    fn the_watts_scale_tops_out_in_fives_above_the_most_drawn() {
        let s = [
            sample(0, 3.2, Charge::Discharging),
            sample(1, 9.1, Charge::Discharging),
        ];
        assert_eq!(watts_top(&s), 15.0);
        assert_eq!(watts_top(&[]), 5.0);
    }

    #[test]
    fn the_average_draw_counts_only_time_on_battery_in_the_span() {
        let s = [
            sample(10, 50.0, Charge::Discharging),
            sample(100, 4.0, Charge::Discharging),
            sample(110, 20.0, Charge::Charging),
            sample(120, 8.0, Charge::Discharging),
        ];
        assert_eq!(average_draw(&s, 50), Some(6.0));
        assert_eq!(average_draw(&s[2..3], 0), None);
    }

    #[test]
    fn plugged_in_stretches_are_one_each() {
        let s = [
            sample(0, 50.0, Charge::Charging),
            sample(10, 60.0, Charge::Charging),
            sample(20, 100.0, Charge::Full),
            sample(30, 99.0, Charge::Discharging),
            sample(40, 90.0, Charge::Charging),
        ];
        assert_eq!(plugged(&s, 50), [(0, 30), (40, 50)]);
    }

    #[test]
    fn a_day_is_marked_every_six_hours_and_a_week_each_day() {
        let to = 1_792_739_825;
        let day = ticks(to - Span::Day.seconds(), to, Span::Day);
        assert_eq!(day.len(), 4);
        assert!(day.iter().all(|(line, named, _)| Some(*line) == *named));
        let week = ticks(to - Span::Week.seconds(), to, Span::Week);
        assert!((6..=7).contains(&week.len()));
    }

    /// A day and a week as nifl's battery might have them: down from full
    /// through the day, charged over lunch and at night.
    fn readings(now: u64) -> Readings {
        let mut charge = Vec::new();
        let mut rate = Vec::new();
        let mut level: f64 = 100.0;
        let mut t = now - Span::Week.seconds();
        while t <= now {
            let hour = (t / 3600) % 24;
            let plugged = !(8..20).contains(&hour) || hour == 13;
            let (state, draw) = if plugged {
                level = (level + 9.0).min(100.0);
                let state = if level >= 100.0 {
                    Charge::Full
                } else {
                    Charge::Charging
                };
                (state, if level >= 100.0 { 0.0 } else { 18.0 })
            } else {
                let draw = 5.0 + ((t / 600) % 7) as f64 * 0.9;
                level = (level - draw / 1.5).max(4.0);
                (Charge::Discharging, draw)
            };
            charge.push(sample(t, level, state));
            rate.push(sample(t, draw, state));
            t += 1800;
        }
        let history = History { charge, rate };
        Readings {
            battery: Some(Battery {
                percent: 65,
                charge: Charge::Discharging,
                minutes: Some(251),
            }),
            health: Some(Health {
                capacity: 80.1,
                cycles: Some(126),
            }),
            history: [Some(Ok(Some(history.clone()))), Some(Ok(Some(history)))],
        }
    }

    /// Draws the section in each built-in skin, a day and a week, as PNGs
    /// in the folder PATINA_SETTINGS_PREVIEW names, from made-up history.
    #[test]
    #[ignore]
    fn preview_the_battery() {
        let dir = std::path::PathBuf::from(std::env::var("PATINA_SETTINGS_PREVIEW").unwrap());
        let (w, h) = (640usize, 520usize);
        let now = 1_792_739_825;
        let readings = readings(now);
        let (skins, _) = patina::skin::load_all(patina::skin::BUILT_IN, &[]);
        for skin in &skins {
            for span in [Span::Day, Span::Week] {
                let ctx = egui::Context::default();
                skin.apply(&ctx);
                let mut painter = patina_shell::raster::Painter::default();
                let mut pixels = vec![0u32; w * h];
                for pass in 0..3 {
                    let mut input = egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(
                            pos2(0.0, 0.0),
                            vec2(w as f32, h as f32),
                        )),
                        max_texture_side: Some(8192),
                        ..Default::default()
                    };
                    // The pointer over the charge graph, for its readout.
                    if pass > 0 {
                        input
                            .events
                            .push(egui::Event::PointerMoved(pos2(w as f32 * 0.6, 160.0)));
                    }
                    let mut span = span;
                    let out = ctx.run_ui(input, |ui| {
                        egui::CentralPanel::default().show(ui, |ui| {
                            show(ui, &readings, &mut span, now);
                        });
                    });
                    let primitives = ctx.tessellate(out.shapes, out.pixels_per_point);
                    painter.paint(
                        &mut pixels,
                        w,
                        h,
                        out.pixels_per_point,
                        &primitives,
                        &out.textures_delta,
                    );
                }
                let rgba: Vec<u8> = pixels
                    .iter()
                    .flat_map(|p| {
                        let [b, g, r, a] = p.to_le_bytes();
                        [r, g, b, a]
                    })
                    .collect();
                let file = dir.join(format!("battery-{}-{span:?}.png", skin.name));
                image::save_buffer(&file, &rgba, w as u32, h as u32, image::ColorType::Rgba8)
                    .unwrap();
            }
        }
    }
}
