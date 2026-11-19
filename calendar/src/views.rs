//! The day, week and month views, and the sidebar's mini month. Each draws
//! occurrences with the painter and reports what the user did; the app
//! acts on it.

use std::collections::HashMap;

use chrono::{Datelike, Duration, NaiveDate, NaiveTime, Timelike};
use chrono_tz::Tz;
use egui::{Align2, Color32, CornerRadius, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2, pos2, vec2};

use crate::ics;
use crate::model::{Event, Occurrence};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum View {
    Day,
    Week,
    Month,
}

/// What the user did in a view.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Opened an event (by index into the events), at one occurrence's start.
    Open(usize, chrono::DateTime<Tz>),
    /// Asked for a new event at this time (or day, when all-day).
    New { start: chrono::DateTime<Tz>, all_day: bool },
    /// Went to a day, in the day view.
    ShowDay(NaiveDate),
}

/// What every view needs to draw.
pub struct Scene<'a> {
    pub events: &'a [Event],
    pub occurrences: &'a [Occurrence],
    pub colors: &'a HashMap<String, Color32>,
    pub local: Tz,
    pub today: NaiveDate,
    pub now: chrono::DateTime<Tz>,
}

impl Scene<'_> {
    fn color(&self, occ: &Occurrence, v: &egui::Visuals) -> Color32 {
        self.colors.get(&self.events[occ.event].calendar).copied().unwrap_or(v.selection.bg_fill)
    }

    fn title(&self, occ: &Occurrence) -> &str {
        let s = self.events[occ.event].summary.as_str();
        if s.is_empty() { "(No title)" } else { s }
    }

    fn on(&self, day: NaiveDate) -> impl Iterator<Item = &Occurrence> {
        self.occurrences.iter().filter(move |o| {
            let (a, b) = o.days();
            a <= day && day <= b
        })
    }
}

/// `#RRGGBB` or `#RRGGBBAA`.
pub fn parse_color(s: &str) -> Option<Color32> {
    let hex = s.trim().strip_prefix('#')?;
    let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
    match hex.len() {
        6 | 8 => Some(Color32::from_rgb(byte(0)?, byte(2)?, byte(4)?)),
        _ => None,
    }
}

/// Black or white, whichever reads better on `bg`.
fn text_on(bg: Color32) -> Color32 {
    let l = 0.299 * bg.r() as f32 + 0.587 * bg.g() as f32 + 0.114 * bg.b() as f32;
    if l > 160.0 { Color32::from_gray(20) } else { Color32::WHITE }
}

/// `color` faded towards the panel, for event backgrounds.
fn tint(color: Color32, v: &egui::Visuals, amount: f32) -> Color32 {
    let bg = v.panel_fill;
    let mix = |a: u8, b: u8| (a as f32 * amount + b as f32 * (1.0 - amount)).round() as u8;
    Color32::from_rgb(mix(color.r(), bg.r()), mix(color.g(), bg.g()), mix(color.b(), bg.b()))
}

fn lines(v: &egui::Visuals) -> Color32 {
    v.widgets.noninteractive.bg_stroke.color
}

fn time_label(t: chrono::DateTime<Tz>) -> String {
    t.format("%H:%M").to_string()
}

pub fn week_start(day: NaiveDate) -> NaiveDate {
    day - Duration::days(day.weekday().num_days_from_monday() as i64)
}

/// The 42 days a month grid shows: six weeks from the Monday on or before
/// the 1st.
pub fn month_grid(day: NaiveDate) -> NaiveDate {
    week_start(day.with_day(1).unwrap())
}

fn day_header(day: NaiveDate) -> String {
    day.format("%a %-d").to_string()
}

/// A pill with text, clipped: all-day events and month entries.
#[allow(clippy::too_many_arguments)]
fn pill(
    ui: &egui::Ui,
    rect: Rect,
    color: Color32,
    text: &str,
    filled: bool,
    id: egui::Id,
    v: &egui::Visuals,
) -> egui::Response {
    let resp = ui.interact(rect, id, Sense::click());
    let painter = ui.painter_at(rect);
    let (bg, fg) = if filled { (color, text_on(color)) } else { (Color32::TRANSPARENT, v.text_color()) };
    if filled {
        painter.rect_filled(rect, CornerRadius::same(4), if resp.hovered() { color.gamma_multiply(0.85) } else { bg });
    } else if resp.hovered() {
        painter.rect_filled(rect, CornerRadius::same(4), v.widgets.hovered.weak_bg_fill);
    }
    let mut x = rect.left() + 5.0;
    if !filled {
        painter.circle_filled(pos2(x + 3.0, rect.center().y), 3.5, color);
        x += 11.0;
    }
    emoji_text(&painter, pos2(x, rect.center().y), Align2::LEFT_CENTER, text, FontId::proportional(12.0), fg);
    resp
}

/// `painter.text` for a left-aligned event title or name, its emoji in colour.
pub fn emoji_text(painter: &egui::Painter, at: Pos2, align: Align2, text: &str, font: FontId, colour: Color32) {
    let line = patina::emoji::line(painter, text, font, colour, f32::INFINITY);
    let top = if align.y() == egui::Align::Center { at.y - line.size().y / 2.0 } else { at.y };
    line.paint(painter, pos2(at.x, top));
}

// ---------------------------------------------------------------------------
// Month
// ---------------------------------------------------------------------------

pub fn month(ui: &mut egui::Ui, scene: &Scene, focus: NaiveDate) -> Option<Action> {
    let v = ui.visuals().clone();
    let mut action = None;
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, Sense::hover());
    let header_h = 24.0;
    let first = month_grid(focus);
    let cell = vec2(area.width() / 7.0, (area.height() - header_h) / 6.0);
    let painter = ui.painter_at(area);
    for (i, name) in ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"].iter().enumerate() {
        let x = area.left() + cell.x * (i as f32 + 0.5);
        painter.text(
            pos2(x, area.top() + header_h / 2.0),
            Align2::CENTER_CENTER,
            *name,
            FontId::proportional(12.0),
            v.weak_text_color(),
        );
    }
    let row_h = 18.0;
    for w in 0..6 {
        for d in 0..7 {
            let day = first + Duration::days(w * 7 + d);
            let rect = Rect::from_min_size(
                pos2(area.left() + cell.x * d as f32, area.top() + header_h + cell.y * w as f32),
                cell,
            );
            let in_month = day.month() == focus.month();
            let id = ui.id().with(("month-cell", day));
            let resp = ui.interact(rect, id, Sense::click());
            if !in_month {
                painter.rect_filled(rect, 0.0, v.faint_bg_color);
            }
            painter.rect_stroke(rect, 0.0, Stroke::new(0.5, lines(&v)), StrokeKind::Inside);
            // The day number; today's in an accent circle. Clicking it opens the day.
            let num_pos = pos2(rect.right() - 14.0, rect.top() + 12.0);
            let num_rect = Rect::from_center_size(num_pos, Vec2::splat(22.0));
            let color = if in_month { v.text_color() } else { v.weak_text_color() };
            if day == scene.today {
                painter.circle_filled(num_pos, 11.0, v.selection.bg_fill);
            }
            let num_color = if day == scene.today { v.selection.stroke.color } else { color };
            painter.text(num_pos, Align2::CENTER_CENTER, day.day().to_string(), FontId::proportional(13.0), num_color);
            if ui
                .interact(num_rect, id.with("num"), Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                action = Some(Action::ShowDay(day));
            }
            // Entries, as many as fit, then "N more".
            let entries: Vec<&Occurrence> = scene.on(day).collect();
            let room = ((rect.height() - 26.0) / row_h).floor().max(0.0) as usize;
            let shown = if entries.len() > room { room.saturating_sub(1) } else { entries.len() };
            for (k, occ) in entries.iter().take(shown).enumerate() {
                let r = Rect::from_min_size(
                    pos2(rect.left() + 3.0, rect.top() + 24.0 + row_h * k as f32),
                    vec2(rect.width() - 6.0, row_h - 2.0),
                );
                let text = if occ.all_day {
                    scene.title(occ).to_string()
                } else {
                    format!("{} {}", time_label(occ.start), scene.title(occ))
                };
                let resp = pill(ui, r, scene.color(occ, &v), &text, occ.all_day, id.with(("entry", k)), &v);
                if resp.clicked() {
                    action = Some(Action::Open(occ.event, occ.start));
                }
            }
            if entries.len() > shown {
                let r = Rect::from_min_size(
                    pos2(rect.left() + 3.0, rect.top() + 24.0 + row_h * shown as f32),
                    vec2(rect.width() - 6.0, row_h - 2.0),
                );
                let more = ui.interact(r, id.with("more"), Sense::click());
                painter.text(
                    pos2(r.left() + 5.0, r.center().y),
                    Align2::LEFT_CENTER,
                    format!("{} more", entries.len() - shown),
                    FontId::proportional(12.0),
                    v.weak_text_color(),
                );
                if more.clicked() {
                    action = Some(Action::ShowDay(day));
                }
            }
            if resp.double_clicked() && action.is_none() {
                action = Some(Action::New { start: ics::local_midnight(day, scene.local), all_day: true });
            }
        }
    }
    action
}

// ---------------------------------------------------------------------------
// Day and week: a time grid
// ---------------------------------------------------------------------------

const HOUR_H: f32 = 44.0;
const GUTTER: f32 = 52.0;

/// Places the timed occurrences of one day side by side where they overlap.
/// Returns (occurrence index, column, columns in its cluster).
fn columns(occs: &[(usize, f32, f32)]) -> Vec<(usize, usize, usize)> {
    let mut out = Vec::new();
    let mut sorted: Vec<&(usize, f32, f32)> = occs.iter().collect();
    sorted.sort_by(|a, b| a.1.total_cmp(&b.1).then(b.2.total_cmp(&a.2)));
    let mut cluster: Vec<(usize, usize, f32)> = Vec::new(); // (index, column, end)
    let mut cluster_end = f32::MIN;
    let flush = |cluster: &mut Vec<(usize, usize, f32)>, out: &mut Vec<(usize, usize, usize)>| {
        let n = cluster.iter().map(|c| c.1 + 1).max().unwrap_or(1);
        out.extend(cluster.drain(..).map(|(i, col, _)| (i, col, n)));
    };
    for &&(i, top, bottom) in &sorted {
        if top >= cluster_end && !cluster.is_empty() {
            flush(&mut cluster, &mut out);
        }
        // The first column free at `top`.
        let mut col = 0;
        while cluster.iter().any(|c| c.1 == col && c.2 > top) {
            col += 1;
        }
        cluster.push((i, col, bottom));
        cluster_end = cluster_end.max(bottom);
    }
    flush(&mut cluster, &mut out);
    out
}

/// The day or week view: `days` columns from `first`. `scroll_to` sets the
/// scroll position once (in hours), e.g. after switching dates.
pub fn time_grid(
    ui: &mut egui::Ui,
    scene: &Scene,
    first: NaiveDate,
    days: usize,
    scroll_to: &mut Option<f32>,
) -> Option<Action> {
    let v = ui.visuals().clone();
    let mut action = None;
    let width = ui.available_width();
    let col_w = (width - GUTTER) / days as f32;
    let day_at = |i: usize| first + Duration::days(i as i64);

    // Header: day names, today highlighted.
    let (header, _) = ui.allocate_exact_size(vec2(width, 28.0), Sense::hover());
    let painter = ui.painter();
    for i in 0..days {
        let day = day_at(i);
        let center = pos2(header.left() + GUTTER + col_w * (i as f32 + 0.5), header.center().y);
        let text = if days == 1 { day.format("%A %-d %B").to_string() } else { day_header(day) };
        let (fg, font) = if day == scene.today {
            (v.selection.bg_fill, FontId::proportional(14.0))
        } else {
            (v.text_color(), FontId::proportional(13.0))
        };
        painter.text(center, Align2::CENTER_CENTER, text, font, fg);
        let r = Rect::from_center_size(center, vec2(col_w, header.height()));
        if days > 1
            && ui
                .interact(r, ui.id().with(("hdr", i)), Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
        {
            action = Some(Action::ShowDay(day));
        }
    }

    // All-day lane: spans laid out in rows.
    let last = day_at(days - 1);
    let mut spans: Vec<(&Occurrence, usize, usize)> = scene
        .occurrences
        .iter()
        .filter(|o| o.all_day || o.days().0 != o.days().1 && (o.end - o.start) >= Duration::hours(24))
        .filter_map(|o| {
            let (a, b) = o.days();
            if b < first || a > last {
                return None;
            }
            let from = (a.max(first) - first).num_days() as usize;
            let to = (b.min(last) - first).num_days() as usize;
            Some((o, from, to))
        })
        .collect();
    spans.sort_by_key(|s| (s.1, std::cmp::Reverse(s.2)));
    let mut rows: Vec<Vec<(usize, usize)>> = Vec::new();
    let mut placed = Vec::new();
    for (o, from, to) in spans {
        let row = rows.iter().position(|r| r.iter().all(|&(a, b)| to < a || from > b)).unwrap_or_else(|| {
            rows.push(Vec::new());
            rows.len() - 1
        });
        rows[row].push((from, to));
        placed.push((o, from, to, row));
    }
    let lane_h = (rows.len().max(1) as f32) * 20.0 + 4.0;
    let (lane, lane_resp) = ui.allocate_exact_size(vec2(width, lane_h), Sense::click());
    ui.painter().text(
        pos2(lane.left() + GUTTER - 6.0, lane.top() + 11.0),
        Align2::RIGHT_CENTER,
        "all-day",
        FontId::proportional(11.0),
        v.weak_text_color(),
    );
    for (k, (o, from, to, row)) in placed.iter().enumerate() {
        let r = Rect::from_min_max(
            pos2(lane.left() + GUTTER + col_w * *from as f32 + 2.0, lane.top() + 2.0 + 20.0 * *row as f32),
            pos2(lane.left() + GUTTER + col_w * (*to + 1) as f32 - 2.0, lane.top() + 20.0 * (*row + 1) as f32),
        );
        if pill(ui, r, scene.color(o, &v), scene.title(o), true, ui.id().with(("allday", k)), &v).clicked() {
            action = Some(Action::Open(o.event, o.start));
        }
    }
    if lane_resp.double_clicked() && action.is_none() {
        let pointer = lane_resp.interact_pointer_pos().unwrap_or(lane.center());
        let i = (((pointer.x - lane.left() - GUTTER) / col_w).floor().max(0.0) as usize).min(days - 1);
        action = Some(Action::New { start: ics::local_midnight(day_at(i), scene.local), all_day: true });
    }
    ui.painter().hline(lane.x_range(), lane.bottom(), Stroke::new(1.0, lines(&v)));

    // The hours.
    let mut scroll = egui::ScrollArea::vertical().auto_shrink(false).id_salt("time-grid");
    if let Some(hour) = scroll_to.take() {
        scroll = scroll.vertical_scroll_offset((hour * HOUR_H).max(0.0));
    }
    scroll.show(ui, |ui| {
        let (grid, grid_resp) = ui.allocate_exact_size(vec2(width, HOUR_H * 24.0), Sense::click());
        let painter = ui.painter_at(grid);
        for h in 0..24 {
            let y = grid.top() + HOUR_H * h as f32;
            painter.hline(grid.left() + GUTTER..=grid.right(), y, Stroke::new(0.6, lines(&v)));
            painter.hline(
                grid.left() + GUTTER..=grid.right(),
                y + HOUR_H / 2.0,
                Stroke::new(0.3, lines(&v).gamma_multiply(0.6)),
            );
            if h > 0 {
                painter.text(
                    pos2(grid.left() + GUTTER - 6.0, y),
                    Align2::RIGHT_CENTER,
                    format!("{h:02}:00"),
                    FontId::proportional(11.0),
                    v.weak_text_color(),
                );
            }
        }
        for i in 0..=days {
            let x = grid.left() + GUTTER + col_w * i as f32;
            painter.vline(x, grid.y_range(), Stroke::new(0.6, lines(&v)));
        }
        let y_of = |t: NaiveTime| grid.top() + (t.num_seconds_from_midnight() as f32 / 3600.0) * HOUR_H;

        for i in 0..days {
            let day = day_at(i);
            let day_start = ics::local_midnight(day, scene.local);
            let day_end = ics::local_midnight(day + Duration::days(1), scene.local);
            // Timed occurrences on this day, clipped to it, in pixels.
            let mut segs = Vec::new();
            for (k, o) in scene.occurrences.iter().enumerate() {
                if o.all_day || (o.end - o.start) >= Duration::hours(24) && o.days().0 != o.days().1 {
                    continue;
                }
                if o.start >= day_end
                    || (o.end <= day_start && o.end != o.start)
                    || (o.end == o.start && o.start < day_start)
                {
                    continue;
                }
                let top = if o.start < day_start { grid.top() } else { y_of(o.start.time()) };
                let bottom = if o.end >= day_end { grid.bottom() } else { y_of(o.end.time()) };
                segs.push((k, top, bottom.max(top + 18.0)));
            }
            let x0 = grid.left() + GUTTER + col_w * i as f32;
            for (k, col, n) in columns(&segs) {
                let (_, top, bottom) = *segs.iter().find(|s| s.0 == k).unwrap();
                let o = &scene.occurrences[k];
                let w = (col_w - 4.0) / n as f32;
                let r = Rect::from_min_max(
                    pos2(x0 + 2.0 + w * col as f32, top + 1.0),
                    pos2(x0 + 2.0 + w * (col + 1) as f32 - 1.0, bottom - 1.0),
                );
                let color = scene.color(o, &v);
                let resp = ui.interact(r, ui.id().with(("ev", k, i)), Sense::click());
                let fill = tint(color, &v, if resp.hovered() { 0.45 } else { 0.3 });
                painter.rect_filled(r, CornerRadius::same(4), fill);
                painter.rect_filled(
                    Rect::from_min_size(r.min, vec2(3.0, r.height())),
                    CornerRadius { nw: 4, sw: 4, ne: 0, se: 0 },
                    color,
                );
                let clip = ui.painter_at(r.shrink2(vec2(4.0, 1.0)));
                let mut y = r.top() + 3.0;
                emoji_text(
                    &clip,
                    pos2(r.left() + 7.0, y),
                    Align2::LEFT_TOP,
                    scene.title(o),
                    FontId::proportional(12.5),
                    v.strong_text_color(),
                );
                y += 15.0;
                if r.height() > 34.0 {
                    let when = format!("{} – {}", time_label(o.start), time_label(o.end));
                    clip.text(
                        pos2(r.left() + 7.0, y),
                        Align2::LEFT_TOP,
                        when,
                        FontId::proportional(11.0),
                        v.text_color(),
                    );
                    y += 14.0;
                    let loc = &scene.events[o.event].location;
                    if r.height() > 50.0 && !loc.is_empty() {
                        clip.text(
                            pos2(r.left() + 7.0, y),
                            Align2::LEFT_TOP,
                            loc,
                            FontId::proportional(11.0),
                            v.weak_text_color(),
                        );
                    }
                }
                if resp.clicked() {
                    action = Some(Action::Open(o.event, o.start));
                }
                resp.on_hover_text(format!("{}\n{} – {}", scene.title(o), time_label(o.start), time_label(o.end)));
            }
            // The current time.
            if day == scene.today {
                let y = y_of(scene.now.time());
                let red = v.error_fg_color;
                painter.hline(x0..=x0 + col_w, y, Stroke::new(1.5, red));
                painter.circle_filled(pos2(x0, y), 4.0, red);
            }
        }
        // Double-click empty time for a new event, on the half hour.
        if grid_resp.double_clicked()
            && action.is_none()
            && let Some(p) = grid_resp.interact_pointer_pos()
        {
            let i = (((p.x - grid.left() - GUTTER) / col_w).floor().max(0.0) as usize).min(days - 1);
            let minutes = (((p.y - grid.top()) / HOUR_H * 60.0) / 30.0).floor() as i64 * 30;
            let start = ics::local_midnight(day_at(i), scene.local) + Duration::minutes(minutes.clamp(0, 23 * 60 + 30));
            action = Some(Action::New { start, all_day: false });
        }
    });
    action
}

// ---------------------------------------------------------------------------
// The sidebar's mini month
// ---------------------------------------------------------------------------

/// A small month for picking a date. `shown` is the month on display and
/// moves with its arrows; days with events get a dot. Returns a clicked day.
pub fn mini_month(
    ui: &mut egui::Ui,
    shown: &mut NaiveDate,
    selected: NaiveDate,
    today: NaiveDate,
    busy_days: &dyn Fn(NaiveDate) -> bool,
) -> Option<NaiveDate> {
    let v = ui.visuals().clone();
    let mut picked = None;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(shown.format("%B %Y").to_string()).strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("›").on_hover_text("Next month").clicked() {
                *shown = (shown.with_day(1).unwrap() + Duration::days(32)).with_day(1).unwrap();
            }
            if ui.small_button("‹").on_hover_text("Previous month").clicked() {
                *shown = (shown.with_day(1).unwrap() - Duration::days(1)).with_day(1).unwrap();
            }
        });
    });
    let width = ui.available_width();
    let cell = width / 7.0;
    let (rect, _) = ui.allocate_exact_size(vec2(width, cell * 0.8 + cell * 6.0), Sense::hover());
    let painter = ui.painter_at(rect);
    for (i, name) in ["M", "T", "W", "T", "F", "S", "S"].iter().enumerate() {
        painter.text(
            pos2(rect.left() + cell * (i as f32 + 0.5), rect.top() + cell * 0.4),
            Align2::CENTER_CENTER,
            *name,
            FontId::proportional(10.5),
            v.weak_text_color(),
        );
    }
    let first = month_grid(*shown);
    for k in 0..42 {
        let day = first + Duration::days(k);
        let center = Pos2::new(
            rect.left() + cell * ((k % 7) as f32 + 0.5),
            rect.top() + cell * 0.8 + cell * ((k / 7) as f32 + 0.5),
        );
        let r = Rect::from_center_size(center, Vec2::splat(cell));
        let resp = ui.interact(r, ui.id().with(("mini", day)), Sense::click());
        let in_month = day.month() == shown.month();
        let mut fg = if in_month { v.text_color() } else { v.weak_text_color() };
        if day == selected {
            painter.circle_filled(center, cell * 0.42, v.widgets.active.weak_bg_fill);
        } else if resp.hovered() {
            painter.circle_filled(center, cell * 0.42, v.widgets.hovered.weak_bg_fill);
        }
        if day == today {
            painter.circle_filled(center, cell * 0.42, v.selection.bg_fill);
            fg = v.selection.stroke.color;
        }
        painter.text(center, Align2::CENTER_CENTER, day.day().to_string(), FontId::proportional(11.5), fg);
        if busy_days(day) && in_month {
            painter.circle_filled(
                center + vec2(0.0, cell * 0.32),
                1.6,
                if day == today { fg } else { v.weak_text_color() },
            );
        }
        if resp.clicked() {
            picked = Some(day);
        }
    }
    picked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlapping_events_share_the_width_and_others_take_it_all() {
        // a and b overlap; c overlaps b; d stands alone.
        let segs = [(0, 0.0, 100.0), (1, 50.0, 150.0), (2, 120.0, 200.0), (3, 300.0, 350.0)];
        let mut placed = columns(&segs);
        placed.sort();
        assert_eq!(placed, [(0, 0, 2), (1, 1, 2), (2, 0, 2), (3, 0, 1)]);
    }

    use crate::model::events_from_ics;

    fn stockholm() -> Tz {
        "Europe/Stockholm".parse().unwrap()
    }

    /// Runs `view` over `events` for one frame per entry in `frames`,
    /// returning the actions it reported.
    fn drive(
        events: &[Event],
        from: NaiveDate,
        to: NaiveDate,
        frames: Vec<Vec<egui::Event>>,
        view: impl Fn(&mut egui::Ui, &Scene) -> Option<Action>,
    ) -> Vec<Action> {
        let ctx = egui::Context::default();
        let occ = crate::model::occurrences(events, from, to, stockholm(), |_| true);
        let colors = HashMap::new();
        let mut out = Vec::new();
        for (n, events_in) in frames.into_iter().enumerate() {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 700.0))),
                time: Some(n as f64 * 0.05),
                events: events_in,
                ..Default::default()
            };
            let _ = ctx.run_ui(input, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    let scene = Scene {
                        events,
                        occurrences: &occ,
                        colors: &colors,
                        local: stockholm(),
                        today: from,
                        now: ics::local_midnight(from, stockholm()),
                    };
                    if let Some(a) = view(ui, &scene) {
                        out.push(a);
                    }
                });
            });
        }
        out
    }

    fn click(p: Pos2) -> Vec<Vec<egui::Event>> {
        let press = |down| egui::Event::PointerButton {
            pos: p,
            button: egui::PointerButton::Primary,
            pressed: down,
            modifiers: Default::default(),
        };
        vec![vec![], vec![egui::Event::PointerMoved(p)], vec![press(true)], vec![press(false)], vec![]]
    }

    fn sample() -> Vec<Event> {
        let text = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:a\r\nSUMMARY:Lunch\r\nDTSTART;TZID=Europe/Stockholm:20261013T120000\r\nDTEND;TZID=Europe/Stockholm:20261013T130000\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        events_from_ics("c", "h", None, text, stockholm())
    }

    #[test]
    fn clicking_an_event_in_the_month_opens_it() {
        let events = sample();
        let focus = NaiveDate::from_ymd_opt(2026, 10, 13).unwrap();
        let first = month_grid(focus);
        // The panel's margins put the grid a little in; Tuesday the 13th is
        // the second column of the third row, and its first entry sits
        // about 33 px below the cell's top.
        let area = Rect::from_min_size(pos2(8.0, 8.0), vec2(984.0, 684.0));
        let cell = vec2(area.width() / 7.0, (area.height() - 24.0) / 6.0);
        let p = pos2(area.left() + cell.x * 1.5, area.top() + 24.0 + cell.y * 2.0 + 33.0);
        let actions = drive(&events, first, first + Duration::days(41), click(p), |ui, s| month(ui, s, focus));
        assert!(matches!(actions.as_slice(), [Action::Open(0, _)]), "{actions:?}");
    }

    #[test]
    fn clicking_a_day_number_shows_that_day() {
        let events = sample();
        let focus = NaiveDate::from_ymd_opt(2026, 10, 13).unwrap();
        let first = month_grid(focus);
        let area = Rect::from_min_size(pos2(8.0, 8.0), vec2(984.0, 684.0));
        let cell = vec2(area.width() / 7.0, (area.height() - 24.0) / 6.0);
        let p = pos2(area.left() + cell.x * 2.0 - 14.0, area.top() + 24.0 + cell.y * 2.0 + 12.0);
        let actions = drive(&events, first, first + Duration::days(41), click(p), |ui, s| month(ui, s, focus));
        assert_eq!(actions, [Action::ShowDay(NaiveDate::from_ymd_opt(2026, 10, 13).unwrap())]);
    }

    #[test]
    fn grids_start_on_mondays() {
        let d = NaiveDate::from_ymd_opt(2026, 10, 15).unwrap();
        assert_eq!(week_start(d), NaiveDate::from_ymd_opt(2026, 10, 12).unwrap());
        // October 2026 starts on a Thursday, so the grid starts on 28 September.
        assert_eq!(month_grid(d), NaiveDate::from_ymd_opt(2026, 9, 28).unwrap());
    }

    #[test]
    fn colours() {
        assert_eq!(parse_color("#0088FF"), Some(Color32::from_rgb(0, 0x88, 0xFF)));
        assert_eq!(parse_color("#ff8c00ff"), Some(Color32::from_rgb(0xFF, 0x8C, 0)));
        assert_eq!(parse_color("blue"), None);
        assert_eq!(text_on(Color32::from_rgb(0xFF, 0xCC, 0)), Color32::from_gray(20));
        assert_eq!(text_on(Color32::from_rgb(0, 0x55, 0xCC)), Color32::WHITE);
    }
}
