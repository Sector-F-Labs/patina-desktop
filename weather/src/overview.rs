//! The overview, as macOS Weather opens: the weather now, the next hours,
//! the days ahead with their range bars, and tiles for the details.

use chrono::{NaiveDateTime, Timelike};
use egui::{Align, Layout, RichText, Ui, vec2};

use crate::forecast::{self, Forecast};
use crate::icons;
use crate::widgets::{self, Action, Body, Scene, card, heading};

/// Wider than this, the days and the tiles sit side by side.
const TWO_COLUMNS: f32 = 860.0;
const TILE: f32 = 172.0;

pub fn show(ui: &mut Ui, s: &Scene) -> Option<Action> {
    let mut action = None;
    let f = s.forecast;
    hero(ui, s);
    ui.add_space(14.0);
    card(ui, |ui| {
        ui.set_width(ui.available_width());
        heading(ui, "Hourly forecast");
        ui.add_space(4.0);
        ui.separator();
        hourly(ui, s);
    });
    ui.add_space(12.0);
    let wide = ui.available_width() >= TWO_COLUMNS;
    if wide {
        ui.horizontal_top(|ui| {
            let left = (ui.available_width() * 0.46).max(360.0);
            ui.vertical(|ui| {
                ui.set_width(left);
                action = daily(ui, s);
            });
            ui.add_space(4.0);
            ui.vertical(|ui| tiles(ui, s, f));
        });
    } else {
        action = daily(ui, s);
        ui.add_space(12.0);
        tiles(ui, s, f);
    }
    action
}

fn hero(ui: &mut Ui, s: &Scene) {
    let f = s.forecast;
    let c = &f.current;
    let today = &f.days[0];
    ui.vertical_centered(|ui| {
        ui.add_space(6.0);
        ui.label(RichText::new(&s.place.name).size(28.0));
        ui.horizontal(|ui| {
            // Centre the icon and the temperature together.
            let width = 64.0 + 8.0 + 120.0;
            ui.add_space(((ui.available_width() - width) / 2.0).max(0.0));
            icons::icon(ui, 64.0, c.code, c.is_day, s.palette);
            ui.label(RichText::new(s.units.degrees(c.temperature)).size(64.0).color(ui.visuals().strong_text_color()));
        });
        ui.label(RichText::new(forecast::describe(c.code)).size(17.0));
        ui.label(
            RichText::new(format!("H:{}  L:{}", s.units.degrees(today.high), s.units.degrees(today.low))).size(15.0),
        );
    });
}

/// One entry in the hourly strip.
enum Slot {
    Hour(usize),
    Sunrise(NaiveDateTime),
    Sunset(NaiveDateTime),
}

/// The next 24 hours left to right, with sunrise and sunset among them.
fn hourly(ui: &mut Ui, s: &Scene) {
    let f = s.forecast;
    let first = f.current_hour();
    let last = (first + 25).min(f.hours.len());
    let mut slots = Vec::new();
    for i in first..last {
        slots.push(Slot::Hour(i));
        let (from, to) = (f.hours[i].time, f.hours[i].time + chrono::Duration::hours(1));
        for d in &f.days {
            if let Some(t) = d.sunrise.filter(|t| *t >= from && *t < to && i > first) {
                slots.push(Slot::Sunrise(t));
            }
            if let Some(t) = d.sunset.filter(|t| *t >= from && *t < to && i > first) {
                slots.push(Slot::Sunset(t));
            }
        }
    }
    egui::ScrollArea::horizontal().id_salt("hourly").show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for slot in slots {
                ui.allocate_ui_with_layout(vec2(54.0, 104.0), Layout::top_down(Align::Center), |ui| {
                    ui.set_width(54.0);
                    match slot {
                        Slot::Hour(i) => {
                            let h = &f.hours[i];
                            let label = if i == first { "Now".to_string() } else { h.time.format("%H").to_string() };
                            ui.label(RichText::new(label).strong());
                            let code = if i == first { f.current.code } else { h.code };
                            icons::icon(ui, 30.0, code, h.is_day, s.palette);
                            match h.precipitation_chance.filter(|c| *c >= 20.0) {
                                Some(c) => {
                                    ui.label(RichText::new(format!("{c:.0}%")).small().color(s.palette.rain));
                                }
                                None => {
                                    ui.label(RichText::new(" ").small());
                                }
                            }
                            let t = if i == first { f.current.temperature } else { h.temperature };
                            ui.label(RichText::new(s.units.degrees(t)).size(16.0));
                        }
                        Slot::Sunrise(t) | Slot::Sunset(t) => {
                            let rise = matches!(slot, Slot::Sunrise(_));
                            ui.label(RichText::new(t.format("%H:%M").to_string()).strong());
                            sun_marker(ui, rise, s);
                            ui.label(RichText::new(" ").small());
                            ui.label(RichText::new(if rise { "Sunrise" } else { "Sunset" }).size(13.0));
                        }
                    }
                });
            }
        });
    });
}

/// A half sun on the horizon with an arrow up or down.
fn sun_marker(ui: &mut Ui, rise: bool, s: &Scene) {
    let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(30.0), egui::Sense::hover());
    let p = ui.painter_at(rect);
    let c = egui::pos2(rect.center().x, rect.center().y + 5.0);
    p.circle_filled(c, 8.0, s.palette.sun);
    for k in 0..5 {
        let a = std::f32::consts::PI * (1.0 + k as f32 / 4.0);
        let d = egui::Vec2::angled(a);
        p.line_segment([c + d * 10.5, c + d * 14.0], egui::Stroke::new(2.0, s.palette.sun));
    }
    // Hide the lower half under the horizon.
    let below = egui::Rect::from_min_max(egui::pos2(rect.left(), c.y), rect.max);
    p.rect_filled(below, 0.0, ui.visuals().faint_bg_color);
    p.line_segment(
        [egui::pos2(rect.left() + 2.0, c.y), egui::pos2(rect.right() - 2.0, c.y)],
        egui::Stroke::new(1.5, ui.visuals().text_color()),
    );
    // Under the horizon: up for sunrise, down for sunset.
    let (tip, dir) = if rise { (c.y + 4.0, -1.0) } else { (c.y + 12.0, 1.0) };
    widgets::arrow_head(&p, egui::pos2(c.x, tip), egui::vec2(0.0, dir), 6.0, ui.visuals().text_color());
}

/// The days ahead, one row each, with a range bar. A click opens the day.
fn daily(ui: &mut Ui, s: &Scene) -> Option<Action> {
    let f = s.forecast;
    let n = s.days.min(f.days.len());
    let span = f.span(n);
    let today = f.today();
    let mut action = None;
    card(ui, |ui| {
        ui.set_width(ui.available_width());
        heading(ui, &format!("{n}-day forecast"));
        ui.add_space(4.0);
        for day in &f.days[..n] {
            ui.separator();
            let row = ui
                .horizontal(|ui| {
                    ui.set_height(34.0);
                    let name = if day.date == today { "Today".to_string() } else { day.date.format("%a").to_string() };
                    ui.add_sized(vec2(52.0, 34.0), egui::Label::new(RichText::new(name).size(16.0)).halign(Align::Min));
                    ui.vertical(|ui| {
                        ui.set_width(34.0);
                        icons::icon(ui, 26.0, day.code, true, s.palette);
                        if let Some(c) = day.precipitation_chance.filter(|c| *c >= 20.0) {
                            ui.add_space(-4.0);
                            ui.label(RichText::new(format!("{c:.0}%")).size(10.0).color(s.palette.rain));
                        }
                    });
                    ui.add_space(6.0);
                    let weak = ui.visuals().weak_text_color();
                    ui.add_sized(
                        vec2(36.0, 34.0),
                        egui::Label::new(RichText::new(s.units.degrees(day.low)).size(16.0).color(weak)),
                    );
                    let bar = (ui.available_width() - 44.0).max(40.0);
                    let now = (day.date == today).then_some(f.current.temperature);
                    ui.vertical(|ui| {
                        ui.add_space(14.0);
                        widgets::range_bar(ui, bar, span, day.low, day.high, now, s.palette);
                    });
                    ui.add_sized(
                        vec2(36.0, 34.0),
                        egui::Label::new(RichText::new(s.units.degrees(day.high)).size(16.0)),
                    );
                })
                .response;
            let row = ui.interact(row.rect, ui.id().with(("day", day.date)), egui::Sense::click());
            if row.hovered() {
                ui.painter().rect_filled(
                    row.rect.expand(2.0),
                    6.0,
                    ui.visuals().widgets.hovered.weak_bg_fill.gamma_multiply(0.5),
                );
            }
            if row.on_hover_text("Show this day in detail").clicked() {
                action = Some(Action::ShowDay(day.date));
            }
        }
    });
    action
}

/// The detail tiles, as many to a row as fit.
fn tiles(ui: &mut Ui, s: &Scene, f: &Forecast) {
    let u = s.units;
    let c = &f.current;
    let today = &f.days[0];
    let hour = &f.hours[f.current_hour()];
    let mut list: Vec<(&str, Body)> = vec![
        (
            "Feels like",
            Box::new(move |ui: &mut Ui| {
                big(ui, &u.degrees(c.feels_like));
                let diff = c.feels_like - c.temperature;
                note(
                    ui,
                    if diff < -1.5 {
                        "Wind is making it feel colder."
                    } else if diff > 1.5 {
                        "Humidity is making it feel warmer."
                    } else {
                        "Similar to the actual temperature."
                    },
                );
            }),
        ),
        (
            "UV index",
            Box::new(move |ui: &mut Ui| {
                big(ui, &format!("{:.0}", c.uv));
                ui.label(RichText::new(forecast::uv_level(c.uv)).size(16.0));
                ui.add_space(4.0);
                widgets::scale_bar(ui, ui.available_width(), &s.palette.uv, c.uv / 11.0);
                ui.add_space(4.0);
                note(
                    ui,
                    &format!("Highest today {:.0}, {}.", today.uv_max, forecast::uv_level(today.uv_max).to_lowercase()),
                );
            }),
        ),
        (
            "Wind",
            Box::new(move |ui: &mut Ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        big(ui, &format!("{:.0}", u.speed(c.wind)));
                        note(ui, u.speed_unit());
                        note(ui, &format!("Gusts {}", u.wind(c.gusts)));
                    });
                    widgets::compass(ui, 84.0, c.wind_direction, s.palette.wind);
                });
                note(ui, &format!("From the {}", forecast::compass(c.wind_direction)));
            }),
        ),
        (
            "Precipitation",
            Box::new(move |ui: &mut Ui| {
                big(ui, &u.rain(c.precipitation));
                note(ui, "in the last hour");
                ui.add_space(6.0);
                note(ui, &format!("{} expected in the next 24 hours.", u.rain(f.next_24h_precipitation())));
            }),
        ),
        (
            "Humidity",
            Box::new(move |ui: &mut Ui| {
                big(ui, &format!("{:.0}%", c.humidity));
                ui.add_space(6.0);
                note(ui, &format!("The dew point is {} right now.", u.degrees(c.dew_point)));
            }),
        ),
        (
            "Visibility",
            Box::new(move |ui: &mut Ui| {
                big(ui, &u.distance(c.visibility));
                ui.add_space(6.0);
                note(
                    ui,
                    match c.visibility {
                        v if v >= 20_000.0 => "Perfectly clear view.",
                        v if v >= 10_000.0 => "Clear view.",
                        v if v >= 4_000.0 => "Haze is affecting visibility.",
                        v if v >= 1_000.0 => "Mist is reducing visibility.",
                        _ => "Fog is greatly reducing visibility.",
                    },
                );
            }),
        ),
        (
            "Pressure",
            Box::new(move |ui: &mut Ui| {
                let later = f.hours.get(f.current_hour() + 3).map(|h| h.pressure).unwrap_or(hour.pressure);
                let trend = match later - hour.pressure {
                    d if d > 0.5 => "Rising",
                    d if d < -0.5 => "Falling",
                    _ => "Steady",
                };
                ui.vertical_centered(|ui| {
                    widgets::dial(ui, 120.0, (c.pressure - 960.0) / 100.0, s.palette.pressure);
                    ui.label(RichText::new(u.pressure_text(c.pressure)).size(18.0));
                    note(ui, trend);
                });
            }),
        ),
    ];
    if let (Some(rise), Some(set)) = (today.sunrise, today.sunset) {
        list.push((
            "Sun",
            Box::new(move |ui: &mut Ui| {
                let now = f.now();
                let (label, at) = if now < rise {
                    ("Sunrise", rise)
                } else if now < set {
                    ("Sunset", set)
                } else {
                    ("Sunrise", f.days.get(1).and_then(|d| d.sunrise).unwrap_or(rise))
                };
                note(ui, label);
                big(ui, &at.format("%H:%M").to_string());
                widgets::sun_arc(ui, ui.available_width(), sun_position(now, rise, set), s.palette);
                let other = if label == "Sunrise" {
                    format!("Sunset {}", set.format("%H:%M"))
                } else {
                    format!("Sunrise {}", rise.format("%H:%M"))
                };
                note(ui, &other);
            }),
        ));
    }
    if let Some(air) = &f.air {
        list.push((
            "Air quality",
            Box::new(move |ui: &mut Ui| {
                big(ui, &format!("{:.0}", air.european_aqi));
                ui.label(RichText::new(forecast::aqi_level(air.european_aqi)).size(16.0));
                ui.add_space(4.0);
                widgets::scale_bar(ui, ui.available_width(), &s.palette.uv, air.european_aqi / 100.0);
                ui.add_space(4.0);
                note(ui, &format!("PM2.5 {:.0} µg/m³, European AQI.", air.pm2_5));
            }),
        ));
    }
    let spacing = ui.spacing().item_spacing.x;
    let per_row = ((ui.available_width() + spacing) / (TILE + spacing)).floor().max(1.0) as usize;
    let width = (ui.available_width() + spacing) / per_row as f32 - spacing;
    for row in list.chunks(per_row) {
        ui.horizontal_top(|ui| {
            for (title, body) in row {
                ui.allocate_ui_with_layout(vec2(width, 0.0), Layout::top_down(Align::Min), |ui| {
                    card(ui, |ui| {
                        ui.set_width(width - 28.0);
                        ui.set_min_height(140.0);
                        heading(ui, title);
                        ui.add_space(2.0);
                        body(ui);
                    });
                });
            }
        });
        ui.add_space(spacing);
    }
}

/// Where the sun is along `widgets::sun_arc`: sunrise at a quarter, sunset at three quarters.
fn sun_position(now: NaiveDateTime, rise: NaiveDateTime, set: NaiveDateTime) -> f32 {
    let minutes = |t: NaiveDateTime| (t.hour() * 60 + t.minute()) as f32;
    let (n, r, s) = (minutes(now), minutes(rise), minutes(set));
    if n < r {
        0.25 * n / r.max(1.0)
    } else if n < s {
        0.25 + 0.5 * (n - r) / (s - r).max(1.0)
    } else {
        0.75 + 0.25 * (n - s) / (1440.0 - s).max(1.0)
    }
}

fn big(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(30.0).color(ui.visuals().strong_text_color()));
}

fn note(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(12.5));
}
