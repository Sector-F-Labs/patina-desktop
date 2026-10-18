//! The week: the days side by side, then graphs across all of them. A click
//! on a day, or on a graph, opens that day.

use egui::{Align, Layout, RichText, Ui, vec2};

use crate::charts::{self, Range};
use crate::forecast;
use crate::icons;
use crate::widgets::{self, Action, Scene, card};

/// The narrowest a day's column gets before the row scrolls.
const COLUMN: f32 = 104.0;

pub fn show(ui: &mut Ui, s: &Scene) -> Option<Action> {
    let f = s.forecast;
    let n = s.days.min(f.days.len());
    let days = &f.days[..n];
    let mut action = None;

    // The days.
    let spacing = ui.spacing().item_spacing.x;
    // Leave room for the vertical scroll bar.
    let width = ((ui.available_width() - 8.0 + spacing) / n as f32 - spacing).max(COLUMN);
    let span = f.span(n);
    let today = f.today();
    egui::ScrollArea::horizontal().id_salt("week-days").show(ui, |ui| {
        ui.horizontal_top(|ui| {
            for day in days {
                let resp = ui
                    .allocate_ui_with_layout(vec2(width, 0.0), Layout::top_down(Align::Center), |ui| {
                        card(ui, |ui| {
                            ui.set_width(width - 28.0);
                            ui.vertical_centered(|ui| {
                                let name =
                                    if day.date == today { "Today".into() } else { day.date.format("%A").to_string() };
                                ui.label(RichText::new(name).strong());
                                ui.label(RichText::new(day.date.format("%-d %b").to_string()).small().weak());
                                ui.add_space(4.0);
                                icons::icon(ui, 44.0, day.code, true, s.palette);
                                ui.label(RichText::new(forecast::describe(day.code)).small());
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    let w = 2.0 * 34.0 + ui.spacing().item_spacing.x;
                                    ui.add_space(((ui.available_width() - w) / 2.0).max(0.0));
                                    ui.label(RichText::new(s.units.degrees(day.high)).size(18.0).strong());
                                    ui.label(RichText::new(s.units.degrees(day.low)).size(18.0).weak());
                                });
                                widgets::range_bar(ui, ui.available_width(), span, day.low, day.high, None, s.palette);
                                ui.add_space(6.0);
                                let chance =
                                    day.precipitation_chance.map(|c| format!("{c:.0}% · ")).unwrap_or_default();
                                ui.label(
                                    RichText::new(format!("{chance}{}", s.units.rain(day.precipitation)))
                                        .small()
                                        .color(s.palette.rain),
                                );
                                ui.label(
                                    RichText::new(format!(
                                        "{} {}",
                                        s.units.wind(day.wind_max),
                                        forecast::compass(day.wind_direction)
                                    ))
                                    .small(),
                                );
                            });
                        })
                        .response
                    })
                    .inner;
                let resp = ui.interact(resp.rect, ui.id().with(("week-day", day.date)), egui::Sense::click());
                if resp.hovered() {
                    ui.painter().rect_stroke(
                        resp.rect,
                        12,
                        ui.visuals().widgets.hovered.bg_stroke,
                        egui::StrokeKind::Inside,
                    );
                }
                if resp.on_hover_text("Show this day in detail").clicked() {
                    action = Some(Action::ShowDay(day.date));
                }
            }
        });
    });
    ui.add_space(12.0);

    // Graphs across the days.
    let first = f.hours.iter().position(|h| h.time.date() == days[0].date).unwrap_or(0);
    let last = f.hours.iter().rposition(|h| h.time.date() == days[n - 1].date).map_or(f.hours.len(), |i| i + 1);
    let range = Range { forecast: f, hours: &f.hours[first..last], group: "week", units: s.units, palette: s.palette };
    let mut clicked = None;
    let u = s.units;
    card(ui, |ui| {
        ui.set_width(ui.available_width());
        charts::title(ui, "Temperature", &format!("{} to {}", u.degrees(span.0), u.degrees(span.1)));
        clicked = clicked.or(charts::temperature(ui, &range, 240.0, true));
    });
    ui.add_space(10.0);
    card(ui, |ui| {
        ui.set_width(ui.available_width());
        let total: f32 = days.iter().map(|d| d.precipitation).sum();
        charts::title(ui, "Chance of precipitation", "");
        clicked = clicked.or(charts::chance(ui, &range, 110.0));
        ui.add_space(6.0);
        charts::title(ui, "Precipitation", &format!("{} in total", u.rain(total)));
        clicked = clicked.or(charts::amount(ui, &range, 110.0));
    });
    ui.add_space(10.0);
    card(ui, |ui| {
        ui.set_width(ui.available_width());
        let max = days.iter().map(|d| d.gusts_max).fold(0.0, f32::max);
        charts::title(ui, &format!("Wind ({})", u.speed_unit()), &format!("Gusts up to {}", u.wind(max)));
        clicked = clicked.or(charts::wind(ui, &range, 150.0));
    });

    if let Some(h) = clicked.and_then(|x| range.hour_at(x)) {
        action = Some(Action::ShowDay(h.time.date()));
    }
    action
}
