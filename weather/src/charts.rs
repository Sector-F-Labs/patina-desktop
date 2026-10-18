//! The graphs, built on egui_plot. Every chart's x axis is hours since the
//! start of the first hour shown, so charts over the same hours line up and
//! share a hover cursor.

use std::sync::Arc;

use chrono::{Duration, NaiveDateTime, Timelike};
use egui::{Color32, RichText, Stroke, Ui};
use egui_plot::{Bar, BarChart, GridMark, Line, LineStyle, Plot, PlotPoint, PlotPoints, Points, Span, Text, VLine};

use crate::forecast::{Forecast, Hour};
use crate::palette::Palette;
use crate::units::Units;
use crate::widgets;

/// The hours one set of linked charts covers.
pub struct Range<'a> {
    pub forecast: &'a Forecast,
    pub hours: &'a [Hour],
    /// Links the hover cursor across the charts.
    pub group: &'a str,
    pub units: Units,
    pub palette: &'a Palette,
}

impl Range<'_> {
    fn origin(&self) -> NaiveDateTime {
        self.hours.first().map(|h| h.time).unwrap_or_default()
    }

    fn span(&self) -> f64 {
        self.hours.len().max(1) as f64
    }

    fn week(&self) -> bool {
        self.hours.len() > 36
    }

    /// The x of a moment.
    pub fn x(&self, t: NaiveDateTime) -> f64 {
        (t - self.origin()).num_minutes() as f64 / 60.0
    }

    /// The hour under an x, if there is one.
    pub fn hour_at(&self, x: f64) -> Option<&Hour> {
        let i = x.round();
        (i >= 0.0).then(|| self.hours.get(i as usize)).flatten()
    }

    pub fn x_series(&self, f: impl Fn(&Hour) -> f32) -> Vec<[f64; 2]> {
        self.hours.iter().enumerate().map(|(i, h)| [i as f64, f(h) as f64]).collect()
    }

    /// A plot with this range's look: no zooming or panning, hours along
    /// the bottom, night shaded, and a label for the hour under the cursor.
    pub fn plot<'b>(&'b self, id: &str, height: f32, label: impl Fn(&Hour) -> String + 'b) -> Plot<'b> {
        let week = self.week();
        let origin = self.origin();
        Plot::new(format!("{}-{id}", self.group))
            .height(height)
            .allow_zoom(false)
            .allow_drag(false)
            .allow_scroll(false)
            .allow_boxed_zoom(false)
            .allow_double_click_reset(false)
            .show_background(false)
            .show_grid([true, true])
            .link_cursor(egui::Id::new(self.group), [true, false])
            .include_x(0.0)
            .include_x(self.span() - 1.0)
            .set_margin_fraction(egui::vec2(0.0, 0.08))
            .y_axis_min_width(36.0)
            .x_grid_spacer(move |input| {
                let (lo, hi) = input.bounds;
                let (big, small) = if week { (24.0, 12.0) } else { (6.0, 3.0) };
                let mut marks = Vec::new();
                let mut v = (lo / small).ceil() * small;
                while v <= hi {
                    let step = if (v / big).fract().abs() < 1e-6 { big } else { small };
                    marks.push(GridMark { value: v, step_size: step });
                    v += small;
                }
                marks
            })
            .x_axis_formatter(move |mark, _| {
                let t = origin + Duration::minutes((mark.value * 60.0).round() as i64);
                if week {
                    if t.hour() == 0 {
                        t.format("%a %-d").to_string()
                    } else if t.hour() == 12 {
                        "12".into()
                    } else {
                        String::new()
                    }
                } else if t.hour().is_multiple_of(3) {
                    t.format("%H").to_string()
                } else {
                    String::new()
                }
            })
            .label_formatter(move |pos| {
                let x = match pos {
                    egui_plot::HoverPosition::NearDataPoint { position, .. } => position.x,
                    egui_plot::HoverPosition::Elsewhere { position } => position.x,
                };
                let hour = self.hour_at(x)?;
                Some(format!("{}\n{}", hour.time.format(if week { "%a %H:%M" } else { "%H:%M" }), label(hour)))
            })
    }

    /// Shades the nights and marks midnight and now. Call first, so the
    /// data draws over them.
    pub fn backdrop(&self, plot: &mut egui_plot::PlotUi<'_>, ui_visuals: &egui::Visuals) {
        let night = ui_visuals.text_color().gamma_multiply(if ui_visuals.dark_mode { 0.05 } else { 0.06 });
        let (start, end) = (0.0, self.span() - 1.0);
        let mut dark_from = start;
        for day in &self.forecast.days {
            let (Some(rise), Some(set)) = (day.sunrise, day.sunset) else { continue };
            let (rise, set) = (self.x(rise), self.x(set));
            if set < start || rise > end + 24.0 {
                dark_from = set.max(start);
                continue;
            }
            if rise > dark_from {
                plot.add(Span::new("", dark_from.max(start)..=rise.min(end)).fill(night).border_width(0.0));
            }
            dark_from = set;
        }
        if dark_from < end {
            plot.add(Span::new("", dark_from..=end).fill(night).border_width(0.0));
        }
        let now = self.x(self.forecast.now());
        if (start..=end).contains(&now) {
            plot.add(
                VLine::new("Now", now)
                    .stroke(Stroke::new(1.0, ui_visuals.text_color().gamma_multiply(0.6)))
                    .style(LineStyle::dashed_dense())
                    .allow_hover(false),
            );
        }
    }
}

/// A summary line over a chart: its title, then a few figures.
pub fn title(ui: &mut Ui, title: &str, detail: &str) {
    ui.horizontal(|ui| {
        widgets::heading(ui, title);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(egui::RichText::new(detail).small().color(ui.visuals().weak_text_color()));
        });
    });
}

/// Temperature and feels-like, coloured along the temperature scale, with
/// the highs and lows marked. Returns the x clicked, if any.
pub fn temperature(ui: &mut Ui, r: &Range, height: f32, icons: bool) -> Option<f64> {
    let u = r.units;
    let p = r.palette.clone();
    let fahrenheit = u.temperature == crate::units::Temperature::Fahrenheit;
    let temps = r.x_series(|h| u.temp(h.temperature));
    let feels = r.x_series(|h| u.temp(h.feels_like));
    let lowest = temps.iter().chain(&feels).map(|p| p[1]).fold(f64::INFINITY, f64::min);
    let highest = temps.iter().chain(&feels).map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max);
    let floor = (lowest - 2.0).floor();
    // Leave room above for the icons and the high's label.
    let ceiling = highest + (highest - floor).max(4.0) * if icons { 0.35 } else { 0.15 };
    let visuals = ui.visuals().clone();
    let weak = visuals.weak_text_color();
    let text = visuals.text_color();

    let resp = r
        .plot("temperature", height, |h| format!("{}  feels {}", u.degrees(h.temperature), u.degrees(h.feels_like)))
        .include_y(floor)
        .include_y(ceiling)
        .y_axis_formatter(|m, _| format!("{:.0}°", m.value))
        .show(ui, |plot| {
            r.backdrop(plot, &visuals);
            let colour = move |pt: PlotPoint| {
                let c = if fahrenheit { (pt.y as f32 - 32.0) * 5.0 / 9.0 } else { pt.y as f32 };
                p.temp(c)
            };
            plot.line(
                Line::new("Feels like", PlotPoints::from(feels))
                    .color(weak)
                    .style(LineStyle::dashed_loose())
                    .width(1.2),
            );
            plot.line(
                Line::new("Temperature", PlotPoints::from(temps.clone()))
                    .gradient_color(Arc::new(colour), true)
                    .fill(floor as f32)
                    .fill_alpha(0.18)
                    .width(2.5),
            );
            // Each day's high and low.
            let mut marks = Vec::new();
            for day in &r.forecast.days {
                let first = r.hours.iter().position(|h| h.time.date() == day.date);
                let Some(first) = first else { continue };
                let n = r.hours[first..].iter().take_while(|h| h.time.date() == day.date).count();
                let slice = &temps[first..first + n];
                let hi = slice.iter().copied().fold([0.0, f64::NEG_INFINITY], |a, b| if b[1] > a[1] { b } else { a });
                let lo = slice.iter().copied().fold([0.0, f64::INFINITY], |a, b| if b[1] < a[1] { b } else { a });
                marks.push((hi, egui::Align2::CENTER_BOTTOM, "H"));
                marks.push((lo, egui::Align2::CENTER_TOP, "L"));
            }
            for (pt, anchor, kind) in &marks {
                plot.points(Points::new("", vec![*pt]).radius(3.5).color(text).allow_hover(false));
                let off = if *anchor == egui::Align2::CENTER_BOTTOM { 0.6 } else { -0.6 };
                plot.text(
                    Text::new(
                        "",
                        PlotPoint::new(pt[0], pt[1] + off),
                        RichText::new(format!("{kind} {:.0}°", pt[1])).size(11.5),
                    )
                    .anchor(*anchor)
                    .color(text)
                    .allow_hover(false),
                );
            }
        });

    // The weather along the top, every few hours, painted over the plot.
    if icons {
        let painter = ui.painter_at(resp.response.rect);
        let step = if r.week() { 6 } else { 2 };
        let size = if r.week() { 18.0 } else { 22.0 };
        for (i, h) in r.hours.iter().enumerate().skip(1).step_by(step) {
            let x = resp.transform.position_from_point(&PlotPoint::new(i as f64, ceiling)).x;
            let rect = egui::Rect::from_center_size(
                egui::pos2(x, resp.transform.frame().top() + size * 0.7),
                egui::Vec2::splat(size),
            );
            crate::icons::paint(&painter, rect, h.code, h.is_day, r.palette);
        }
    }
    clicked_x(&resp)
}

fn clicked_x<R>(resp: &egui_plot::PlotResponse<R>) -> Option<f64> {
    if resp.response.clicked() {
        let pos = resp.response.interact_pointer_pos()?;
        Some(resp.transform.value_from_position(pos).x)
    } else {
        None
    }
}

/// Chance of precipitation per hour, as bars.
pub fn chance(ui: &mut Ui, r: &Range, height: f32) -> Option<f64> {
    let rain = r.palette.rain;
    let visuals = ui.visuals().clone();
    let bars: Vec<Bar> = r
        .hours
        .iter()
        .enumerate()
        .filter_map(|(i, h)| {
            let c = h.precipitation_chance?;
            Some(Bar::new(i as f64, c as f64).fill(rain.gamma_multiply(0.25 + 0.6 * c / 100.0)).stroke(Stroke::NONE))
        })
        .collect();
    let resp = r
        .plot("chance", height, |h| match h.precipitation_chance {
            Some(c) => format!("{c:.0}% chance"),
            None => "No chance given".into(),
        })
        .include_y(0.0)
        .include_y(100.0)
        .y_axis_formatter(|m, _| format!("{:.0}%", m.value))
        .show(ui, |plot| {
            r.backdrop(plot, &visuals);
            plot.bar_chart(BarChart::new("Chance", bars).width(if r.week() { 1.0 } else { 0.7 }).allow_hover(false));
        });
    clicked_x(&resp)
}

/// Precipitation per hour, rain and snow stacked.
pub fn amount(ui: &mut Ui, r: &Range, height: f32) -> Option<f64> {
    let u = r.units;
    let (rain_c, snow_c) = (r.palette.rain, r.palette.snow);
    let visuals = ui.visuals().clone();
    let width = if r.week() { 1.0 } else { 0.7 };
    let rain: Vec<Bar> = r
        .hours
        .iter()
        .enumerate()
        .map(|(i, h)| Bar::new(i as f64, u.precip(h.precipitation - h.snow) as f64).fill(rain_c).stroke(Stroke::NONE))
        .collect();
    let snow: Vec<Bar> = r
        .hours
        .iter()
        .enumerate()
        .map(|(i, h)| Bar::new(i as f64, u.precip(h.snow) as f64).fill(snow_c).stroke(Stroke::NONE))
        .collect();
    let small = if u.precip_unit() == "mm" { 1.0 } else { 0.04 };
    let unit = u.precip_unit();
    let resp = r
        .plot("amount", height, |h| {
            if h.snow > 0.0 {
                format!("{} ({} as snow)", u.rain(h.precipitation), u.rain(h.snow))
            } else {
                u.rain(h.precipitation)
            }
        })
        .include_y(0.0)
        .include_y(small)
        .y_axis_formatter(move |m, _| format!("{} {unit}", egui_plot::format_number(m.value, 2)))
        .show(ui, |plot| {
            r.backdrop(plot, &visuals);
            let rain = BarChart::new("Rain", rain).width(width).allow_hover(false);
            let snow = BarChart::new("Snow", snow).width(width).stack_on(&[&rain]).allow_hover(false);
            plot.bar_chart(rain);
            plot.bar_chart(snow);
        });
    clicked_x(&resp)
}

/// Wind speed and gusts, with arrows for the direction it blows.
pub fn wind(ui: &mut Ui, r: &Range, height: f32) -> Option<f64> {
    let u = r.units;
    let color = r.palette.wind;
    let visuals = ui.visuals().clone();
    let speed = r.x_series(|h| u.speed(h.wind));
    let gusts = r.x_series(|h| u.speed(h.gusts));
    let top = gusts.iter().map(|p| p[1]).fold(5.0, f64::max);
    let resp = r
        .plot("wind", height, |h| {
            format!("{} {}, gusts {}", u.wind(h.wind), crate::forecast::compass(h.wind_direction), u.wind(h.gusts))
        })
        .include_y(0.0)
        .include_y(top * 1.25)
        .y_axis_formatter(|m, _| format!("{:.0}", m.value))
        .show(ui, |plot| {
            r.backdrop(plot, &visuals);
            plot.line(
                Line::new("Gusts", PlotPoints::from(gusts))
                    .color(color.gamma_multiply(0.5))
                    .style(LineStyle::dashed_loose()),
            );
            plot.line(Line::new("Wind", PlotPoints::from(speed)).color(color).fill(0.0).fill_alpha(0.12).width(2.0));
        });
    // Direction arrows along the top, in screen space so they aren't skewed.
    let painter = ui.painter_at(resp.response.rect);
    let step = if r.week() { 6 } else { 2 };
    for (i, h) in r.hours.iter().enumerate().skip(1).step_by(step) {
        let x = resp.transform.position_from_point(&PlotPoint::new(i as f64, 0.0)).x;
        let c = egui::pos2(x, resp.transform.frame().top() + 9.0);
        let dir = widgets::bearing(h.wind_direction + 180.0);
        painter.line_segment([c - dir * 6.0, c + dir * 3.0], Stroke::new(1.5, visuals.text_color()));
        widgets::arrow_head(&painter, c + dir * 7.0, dir, 5.0, visuals.text_color());
    }
    clicked_x(&resp)
}

/// A single series as a line, for humidity, cloud cover, pressure and visibility.
pub struct Series<'f> {
    pub id: &'static str,
    pub color: Color32,
    pub value: &'f dyn Fn(&Hour) -> f32,
    pub label: &'f dyn Fn(&Hour) -> String,
    pub unit: &'static str,
    /// Fixed y bounds, e.g. 0..100 for percentages.
    pub bounds: Option<(f64, f64)>,
    pub fill: bool,
}

pub fn series(ui: &mut Ui, r: &Range, height: f32, s: Series) -> Option<f64> {
    let visuals = ui.visuals().clone();
    let pts = r.x_series(s.value);
    let mut plot = r.plot(s.id, height, s.label);
    let lowest = pts.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
    let reference = match s.bounds {
        Some((lo, hi)) => {
            plot = plot.include_y(lo).include_y(hi);
            lo
        }
        None => lowest,
    };
    let unit = s.unit;
    let resp =
        plot.y_axis_formatter(move |m, _| format!("{}{unit}", egui_plot::format_number(m.value, 2))).show(ui, |plot| {
            r.backdrop(plot, &visuals);
            let mut line = Line::new(s.id, PlotPoints::from(pts)).color(s.color).width(2.0);
            if s.fill {
                line = line.fill(reference as f32).fill_alpha(0.15);
            }
            plot.line(line);
        });
    clicked_x(&resp)
}

/// The UV index per hour, as bars in its scale's colours.
pub fn uv(ui: &mut Ui, r: &Range, height: f32) -> Option<f64> {
    let p = r.palette;
    let visuals = ui.visuals().clone();
    let bars: Vec<Bar> = r
        .hours
        .iter()
        .enumerate()
        .map(|(i, h)| Bar::new(i as f64, h.uv as f64).fill(p.uv(h.uv)).stroke(Stroke::NONE))
        .collect();
    let resp = r
        .plot("uv", height, |h| format!("UV {:.0}, {}", h.uv, crate::forecast::uv_level(h.uv)))
        .include_y(0.0)
        .include_y(6.0)
        .y_axis_formatter(|m, _| format!("{:.0}", m.value))
        .show(ui, |plot| {
            r.backdrop(plot, &visuals);
            plot.bar_chart(BarChart::new("UV", bars).width(if r.week() { 1.0 } else { 0.7 }).allow_hover(false));
        });
    clicked_x(&resp)
}
