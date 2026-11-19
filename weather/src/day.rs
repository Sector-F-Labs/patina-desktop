//! One day in detail: a strip to pick the day, a summary, then a graph for
//! each kind of weather over its hours.

use chrono::NaiveDate;
use egui::{Align, Layout, RichText, Ui, vec2};

use crate::charts::{self, Range, Series};
use crate::forecast::{self, Day};
use crate::icons;
use crate::widgets::{Action, Body, Scene, card, heading};

/// Wider than this, the smaller graphs go two to a row.
const TWO_COLUMNS: f32 = 760.0;

pub fn show(ui: &mut Ui, s: &Scene, date: NaiveDate) -> Option<Action> {
    let f = s.forecast;
    let action = picker(ui, s, date);
    let Some(day) = f.days.iter().find(|d| d.date == date) else {
        ui.label("This day isn't in the forecast.");
        return action;
    };
    ui.add_space(10.0);
    summary(ui, s, day);
    ui.add_space(10.0);

    let hours = f.hours_of(date);
    let range = Range {
        forecast: f,
        hours,
        group: "day",
        units: s.units,
        palette: s.palette,
        sun: Some((s.place.latitude, s.place.longitude)),
    };
    let u = s.units;
    card(ui, |ui| {
        ui.set_width(ui.available_width());
        charts::title(
            ui,
            "Temperature",
            &format!(
                "High {}  Low {}  Feels {} to {}",
                u.degrees(day.high),
                u.degrees(day.low),
                u.degrees(day.feels_low),
                u.degrees(day.feels_high)
            ),
        );
        charts::temperature(ui, &range, 250.0, true);
    });
    ui.add_space(10.0);

    let p = s.palette;
    let pressure_value = |h: &forecast::Hour| u.pressure(h.pressure);
    let pressure_label = |h: &forecast::Hour| u.pressure_text(h.pressure);
    let humidity_value = |h: &forecast::Hour| h.humidity;
    let humidity_label = |h: &forecast::Hour| format!("{:.0}%, dew point {}", h.humidity, u.degrees(h.dew_point));
    let cloud_value = |h: &forecast::Hour| h.cloud_cover;
    let cloud_label = |h: &forecast::Hour| format!("{:.0}% cloud", h.cloud_cover);
    let vis_value = |h: &forecast::Hour| match u.length {
        crate::units::Length::Metric => h.visibility / 1000.0,
        crate::units::Length::Imperial => h.visibility / 1609.344,
    };
    let vis_label = |h: &forecast::Hour| u.distance(h.visibility);
    let km = if u.length == crate::units::Length::Metric { " km" } else { " mi" };
    let pressure_unit = if u.length == crate::units::Length::Metric { "" } else { "\"" };

    let max_uv = day.uv_max;
    let avg = |v: &dyn Fn(&forecast::Hour) -> f32| hours.iter().map(v).sum::<f32>() / hours.len().max(1) as f32;
    let charts: Vec<(String, String, Body)> = vec![
        (
            "Chance of precipitation".into(),
            day.precipitation_chance.map(|c| format!("Up to {c:.0}%")).unwrap_or_default(),
            Box::new(|ui: &mut Ui| {
                charts::chance(ui, &range, 140.0);
            }),
        ),
        (
            "Precipitation".into(),
            format!("{} over {:.0} h", u.rain(day.precipitation), day.precipitation_hours),
            Box::new(|ui: &mut Ui| {
                charts::amount(ui, &range, 140.0);
            }),
        ),
        (
            format!("Wind ({})", u.speed_unit()),
            format!("Up to {}, gusts {}", u.wind(day.wind_max), u.wind(day.gusts_max)),
            Box::new(|ui: &mut Ui| {
                charts::wind(ui, &range, 140.0);
            }),
        ),
        (
            "UV index".into(),
            format!("Up to {:.0}, {}", max_uv, forecast::uv_level(max_uv)),
            Box::new(|ui: &mut Ui| {
                charts::uv(ui, &range, 140.0);
            }),
        ),
        (
            "Humidity".into(),
            format!("Average {:.0}%", avg(&humidity_value)),
            Box::new(|ui: &mut Ui| {
                charts::series(
                    ui,
                    &range,
                    140.0,
                    Series {
                        id: "humidity",
                        color: p.humidity,
                        value: &humidity_value,
                        label: &humidity_label,
                        unit: "%",
                        bounds: Some((0.0, 100.0)),
                        fill: true,
                    },
                );
            }),
        ),
        (
            "Cloud cover".into(),
            format!("Average {:.0}%", avg(&cloud_value)),
            Box::new(|ui: &mut Ui| {
                charts::series(
                    ui,
                    &range,
                    140.0,
                    Series {
                        id: "cloud",
                        color: p.cloud,
                        value: &cloud_value,
                        label: &cloud_label,
                        unit: "%",
                        bounds: Some((0.0, 100.0)),
                        fill: true,
                    },
                );
            }),
        ),
        (
            format!("Pressure ({})", u.pressure_unit()),
            pressure_trend(hours),
            Box::new(|ui: &mut Ui| {
                charts::series(
                    ui,
                    &range,
                    140.0,
                    Series {
                        id: "pressure",
                        color: p.pressure,
                        value: &pressure_value,
                        label: &pressure_label,
                        unit: pressure_unit,
                        bounds: None,
                        fill: false,
                    },
                );
            }),
        ),
        (
            "Visibility".into(),
            format!("Lowest {}", u.distance(hours.iter().map(|h| h.visibility).fold(f32::INFINITY, f32::min))),
            Box::new(|ui: &mut Ui| {
                charts::series(
                    ui,
                    &range,
                    140.0,
                    Series {
                        id: "visibility",
                        color: p.cloud,
                        value: &vis_value,
                        label: &vis_label,
                        unit: km,
                        bounds: Some((0.0, 0.0)),
                        fill: true,
                    },
                );
            }),
        ),
    ];

    let columns = if ui.available_width() >= TWO_COLUMNS { 2 } else { 1 };
    for row in charts.chunks(columns) {
        ui.columns(columns, |cols| {
            for (col, (title, detail, body)) in cols.iter_mut().zip(row) {
                card(col, |ui| {
                    ui.set_width(ui.available_width());
                    charts::title(ui, title, detail);
                    body(ui);
                });
            }
        });
        ui.add_space(10.0);
    }

    action
}

/// The days of the forecast as a row of buttons, like macOS Weather's
/// date strip.
fn picker(ui: &mut Ui, s: &Scene, date: NaiveDate) -> Option<Action> {
    let f = s.forecast;
    let mut action = None;
    egui::ScrollArea::horizontal().id_salt("day-picker").show(ui, |ui| {
        ui.horizontal(|ui| {
            for day in &f.days {
                let selected = day.date == date;
                let resp = ui
                    .allocate_ui_with_layout(vec2(56.0, 72.0), Layout::top_down(Align::Center), |ui| {
                        ui.set_width(56.0);
                        let weak = ui.visuals().weak_text_color();
                        ui.label(RichText::new(day.date.format("%a").to_string()).small().color(weak));
                        let (rect, _) = ui.allocate_exact_size(vec2(32.0, 32.0), egui::Sense::hover());
                        let v = ui.visuals();
                        if selected {
                            ui.painter().circle_filled(rect.center(), 16.0, v.selection.bg_fill);
                        } else if day.date == f.today() {
                            ui.painter().circle_stroke(rect.center(), 15.0, v.selection.stroke);
                        }
                        let color = if selected { v.selection.stroke.color } else { v.text_color() };
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            day.date.format("%-d").to_string(),
                            egui::FontId::proportional(15.0),
                            color,
                        );
                        icons::icon(ui, 20.0, day.code, true, s.palette);
                    })
                    .response;
                let resp = ui.interact(resp.rect, ui.id().with(("pick", day.date)), egui::Sense::click());
                if resp.hovered() && !selected {
                    ui.painter().rect_filled(
                        resp.rect,
                        8.0,
                        ui.visuals().widgets.hovered.weak_bg_fill.gamma_multiply(0.4),
                    );
                }
                if resp.clicked() {
                    action = Some(Action::ShowDay(day.date));
                }
            }
        });
    });
    action
}

fn summary(ui: &mut Ui, s: &Scene, day: &Day) {
    let u = s.units;
    card(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            icons::icon(ui, 72.0, day.code, true, s.palette);
            ui.add_space(8.0);
            ui.vertical(|ui| {
                ui.label(RichText::new(day.date.format("%A %-d %B").to_string()).size(20.0).strong());
                ui.label(RichText::new(forecast::describe(day.code)).size(16.0));
                ui.label(RichText::new(sentence(s, day)).weak());
            });
        });
        ui.add_space(8.0);
        ui.separator();
        let facts: Vec<(&str, String)> = vec![
            ("High", u.degrees(day.high)),
            ("Low", u.degrees(day.low)),
            ("Feels like", format!("{} to {}", u.degrees(day.feels_low), u.degrees(day.feels_high))),
            ("Sunrise", day.sunrise.map(|t| t.format("%H:%M").to_string()).unwrap_or("—".into())),
            ("Sunset", day.sunset.map(|t| t.format("%H:%M").to_string()).unwrap_or("—".into())),
            (
                "Daylight",
                format!("{}h {:02}m", (day.daylight / 3600.0) as u32, ((day.daylight % 3600.0) / 60.0) as u32),
            ),
            ("UV max", format!("{:.0}", day.uv_max)),
            ("Rain", u.rain(day.precipitation)),
            ("Wind", format!("{} {}", u.wind(day.wind_max), forecast::compass(day.wind_direction))),
        ];
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 18.0;
            for (label, value) in facts {
                ui.allocate_ui_with_layout(vec2(92.0, 40.0), Layout::top_down(Align::Min), |ui| {
                    ui.set_min_width(72.0);
                    heading(ui, label);
                    ui.label(RichText::new(value).size(16.0));
                });
            }
        });
    });
}

/// A sentence about the day, like a forecaster's.
fn sentence(s: &Scene, day: &Day) -> String {
    let u = s.units;
    let f = s.forecast;
    let hours = f.hours_of(day.date);
    let mut parts = vec![format!("Highs of {}, lows of {}.", u.degrees(day.high), u.degrees(day.low))];
    let wet: Vec<_> = hours.iter().filter(|h| h.precipitation_chance.unwrap_or(0.0) >= 50.0).collect();
    if let (Some(first), Some(last)) = (wet.first(), wet.last()) {
        let kind = if wet.iter().any(|h| h.snow > 0.0) { "Snow" } else { "Rain" };
        parts.push(format!(
            "{kind} likely from {} to {}, {} in all.",
            first.time.format("%H:%M"),
            (last.time + chrono::Duration::hours(1)).format("%H:%M"),
            u.rain(day.precipitation)
        ));
    } else if day.precipitation_chance.unwrap_or(0.0) < 20.0 {
        parts.push("Dry.".into());
    }
    if day.gusts_max >= 50.0 {
        parts.push(format!("Gusts up to {}.", u.wind(day.gusts_max)));
    }
    parts.join(" ")
}

fn pressure_trend(hours: &[forecast::Hour]) -> String {
    let (Some(a), Some(b)) = (hours.first(), hours.last()) else { return String::new() };
    match b.pressure - a.pressure {
        d if d > 2.0 => "Rising".into(),
        d if d < -2.0 => "Falling".into(),
        _ => "Steady".into(),
    }
}
