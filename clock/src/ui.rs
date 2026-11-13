//! The clock's menu: today's date, a month with week numbers, today marked
//! and a dot under days with something on, and the chosen day's events
//! from Calendar. A click on a day chooses it, twice opens Calendar there;
//! a click on an event opens it in Calendar. Arrows move the day, Page Up
//! and Down the month, Enter opens Calendar; Escape or a click elsewhere
//! closes it.

use std::collections::BTreeSet;
use std::sync::mpsc;

use chrono::{Datelike, Duration, Local, Months, NaiveDate};
use egui::{
    Align, Align2, Color32, Key, Layout, RichText, ScrollArea, Sense, StrokeKind, Ui, UiBuilder,
    pos2, vec2,
};
use patina::skin::Skin;
use patina_shell::Shell;

use crate::agenda::{self, Item};

const MARGIN: f32 = 14.0;
/// A day's cell in the month.
const CELL: f32 = 34.0;
const WEEK_COLUMN: f32 = 26.0;

/// What the menu asks of the program.
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    /// The events on `days` days from a day, to show a month.
    Agenda(NaiveDate, u32),
    /// Run this through sh(1): Calendar, opened somewhere.
    Run(String),
}

pub struct ClockMenu {
    today: NaiveDate,
    /// The day chosen; the month shown is its month.
    chosen: NaiveDate,
    /// The events of the month shown, from the first day of its grid; or
    /// why there are none.
    pub agenda: Option<(NaiveDate, Result<Vec<Item>, String>)>,
    busy: BTreeSet<NaiveDate>,
    /// The grid asked for last, so each is asked for once.
    asked: Option<NaiveDate>,
    had_focus: bool,
    done: bool,
    skin: Skin,
    requests: mpsc::Sender<Request>,
    /// What day it is: the local one, or a fixed one for previews.
    clock: fn() -> NaiveDate,
}

fn local_today() -> NaiveDate {
    Local::now().date_naive()
}

/// The Monday on or before the 1st of `day`'s month: where its grid of six
/// weeks starts.
pub fn grid_start(day: NaiveDate) -> NaiveDate {
    let first = day.with_day(1).unwrap_or(day);
    first - Duration::days(i64::from(first.weekday().num_days_from_monday()))
}

/// Days in a grid: six weeks, so the menu keeps its height month to month.
pub const GRID_DAYS: u32 = 42;

impl ClockMenu {
    pub fn new(today: NaiveDate, skin: Skin, requests: mpsc::Sender<Request>) -> Self {
        Self {
            today,
            chosen: today,
            agenda: None,
            busy: BTreeSet::new(),
            asked: None,
            had_focus: false,
            done: false,
            skin,
            requests,
            clock: local_today,
        }
    }

    pub fn close(&mut self) {
        self.done = true;
    }

    /// The events for a grid have come.
    pub fn take_agenda(&mut self, from: NaiveDate, items: Result<Vec<Item>, String>) {
        if Some(from) != self.asked {
            return;
        }
        self.busy = items
            .as_ref()
            .map(|i| agenda::busy_days(i))
            .unwrap_or_default();
        self.agenda = Some((from, items));
    }

    /// Chooses `day`, asking for its month's events if they're not here.
    pub fn choose(&mut self, day: NaiveDate) {
        self.chosen = day;
        self.ask();
    }

    fn ask(&mut self) {
        let from = grid_start(self.chosen);
        if self.asked != Some(from) {
            self.asked = Some(from);
            let _ = self.requests.send(Request::Agenda(from, GRID_DAYS));
        }
    }

    /// Opens Calendar at `day`, or at an event on it, and closes.
    fn open(&mut self, day: NaiveDate, event: Option<&str>) {
        let _ = self
            .requests
            .send(Request::Run(agenda::open_command(day, event)));
        self.done = true;
    }

    /// Calendar can be opened: it answered for the events.
    fn calendar_here(&self) -> bool {
        matches!(&self.agenda, Some((_, Ok(_))))
    }

    fn keys(&mut self, ui: &Ui) {
        let step = ui.input(|i| {
            if i.key_pressed(Key::ArrowLeft) {
                Some(Duration::days(-1))
            } else if i.key_pressed(Key::ArrowRight) {
                Some(Duration::days(1))
            } else if i.key_pressed(Key::ArrowUp) {
                Some(Duration::days(-7))
            } else if i.key_pressed(Key::ArrowDown) {
                Some(Duration::days(7))
            } else {
                None
            }
        });
        if let Some(step) = step {
            self.choose(self.chosen + step);
        }
        if ui.input(|i| i.key_pressed(Key::PageUp)) {
            self.choose(self.chosen - Months::new(1));
        }
        if ui.input(|i| i.key_pressed(Key::PageDown)) {
            self.choose(self.chosen + Months::new(1));
        }
        if ui.input(|i| i.key_pressed(Key::Home)) {
            self.choose(self.today);
        }
        if ui.input(|i| i.key_pressed(Key::Enter)) && self.calendar_here() {
            self.open(self.chosen, None);
        }
    }

    /// Today in words, over the year.
    fn header(&self, ui: &mut Ui) {
        ui.label(
            RichText::new(self.today.format("%A").to_string())
                .small()
                .weak(),
        );
        ui.label(RichText::new(self.today.format("%-d %B %Y").to_string()).heading());
    }

    /// The month's name, with arrows either side and Today away from it.
    fn month_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(self.chosen.format("%B %Y").to_string()).strong());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("›").on_hover_text("Next month").clicked() {
                    self.choose(self.chosen + Months::new(1));
                }
                if ui.button("‹").on_hover_text("Previous month").clicked() {
                    self.choose(self.chosen - Months::new(1));
                }
                let away = (self.chosen.year(), self.chosen.month())
                    != (self.today.year(), self.today.month());
                if away && ui.button("Today").clicked() {
                    self.choose(self.today);
                }
            });
        });
    }

    /// The weekdays' letters, then six weeks, each with its ISO number.
    fn month(&mut self, ui: &mut Ui) {
        let v = ui.visuals().clone();
        let small = egui::TextStyle::Small.resolve(ui.style());
        let body = egui::TextStyle::Body.resolve(ui.style());
        let width = WEEK_COLUMN + 7.0 * CELL;
        let (rect, _) = ui.allocate_exact_size(
            vec2(ui.available_width().max(width), 18.0 + 6.0 * CELL),
            Sense::hover(),
        );
        let left = rect.left() + (rect.width() - width) / 2.0;
        let painter = ui.painter();
        for (i, name) in ["M", "T", "W", "T", "F", "S", "S"].iter().enumerate() {
            painter.text(
                pos2(
                    left + WEEK_COLUMN + (i as f32 + 0.5) * CELL,
                    rect.top() + 8.0,
                ),
                Align2::CENTER_CENTER,
                *name,
                small.clone(),
                v.weak_text_color(),
            );
        }
        let start = grid_start(self.chosen);
        let mut chosen = None;
        let mut opened = None;
        for week in 0..6 {
            let top = rect.top() + 18.0 + week as f32 * CELL;
            let monday = start + Duration::weeks(week);
            painter.text(
                pos2(left + WEEK_COLUMN / 2.0, top + CELL / 2.0),
                Align2::CENTER_CENTER,
                monday.iso_week().week().to_string(),
                small.clone(),
                v.weak_text_color(),
            );
            for weekday in 0..7 {
                let day = monday + Duration::days(weekday);
                let cell = egui::Rect::from_min_size(
                    pos2(left + WEEK_COLUMN + weekday as f32 * CELL, top),
                    vec2(CELL, CELL),
                );
                let response = ui.interact(cell, ui.id().with(day), Sense::click());
                let mark = cell.shrink(3.0);
                let is_today = day == self.today;
                let is_chosen = day == self.chosen;
                let in_month = day.month() == self.chosen.month();
                let painter = ui.painter();
                if is_chosen {
                    painter.rect_filled(mark, v.menu_corner_radius, v.selection.bg_fill);
                } else if response.hovered() {
                    painter.rect_filled(mark, v.menu_corner_radius, v.widgets.hovered.weak_bg_fill);
                }
                if is_today && !is_chosen {
                    painter.rect_stroke(
                        mark,
                        v.menu_corner_radius,
                        v.selection.stroke,
                        StrokeKind::Inside,
                    );
                }
                let colour = if is_chosen {
                    v.selection.stroke.color
                } else if !in_month {
                    v.weak_text_color().gamma_multiply(0.7)
                } else if is_today {
                    v.strong_text_color()
                } else {
                    v.text_color()
                };
                painter.text(
                    cell.center() - vec2(0.0, 2.0),
                    Align2::CENTER_CENTER,
                    day.day().to_string(),
                    body.clone(),
                    colour,
                );
                if self.busy.contains(&day) {
                    painter.circle_filled(
                        pos2(cell.center().x, cell.bottom() - 7.0),
                        2.0,
                        colour.gamma_multiply(if is_chosen { 1.0 } else { 0.8 }),
                    );
                }
                if response.double_clicked() {
                    opened = Some(day);
                } else if response.clicked() {
                    chosen = Some(day);
                }
            }
        }
        if let Some(day) = chosen {
            self.choose(day);
        }
        if let Some(day) = opened
            && self.calendar_here()
        {
            self.open(day, None);
        }
    }

    /// The chosen day's events, or why there are none.
    fn events(&mut self, ui: &mut Ui) {
        let day = self.chosen;
        let title = match (day - self.today).num_days() {
            0 => "Today".to_string(),
            1 => "Tomorrow".to_string(),
            -1 => "Yesterday".to_string(),
            _ => day.format("%A %-d %B").to_string(),
        };
        ui.horizontal(|ui| {
            ui.label(RichText::new(title).strong());
            if self.calendar_here() {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.small_button("Open Calendar").clicked() {
                        self.open(day, None);
                    }
                });
            }
        });
        let items: Vec<Item> = match &self.agenda {
            None => return,
            Some((_, Err(e))) => {
                ui.label(RichText::new(e).small().weak());
                return;
            }
            Some((_, Ok(items))) => items.iter().filter(|i| i.on(day)).cloned().collect(),
        };
        if items.is_empty() {
            ui.label(RichText::new("Nothing on").weak());
            return;
        }
        let mut opened = None;
        ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            for item in &items {
                if event_row(ui, item, day).clicked() {
                    opened = Some(item.uid.clone());
                }
            }
        });
        if let Some(uid) = opened {
            self.open(day, Some(&uid));
        }
    }
}

/// An event: its calendar's colour, the time, the title and where.
fn event_row(ui: &mut Ui, item: &Item, day: NaiveDate) -> egui::Response {
    let v = ui.visuals().clone();
    let height = if item.location.is_empty() { 26.0 } else { 40.0 };
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, v.menu_corner_radius, v.widgets.hovered.weak_bg_fill);
    }
    let colour: Color32 = item
        .color
        .as_deref()
        .and_then(agenda::colour)
        .unwrap_or(v.selection.bg_fill);
    ui.painter().rect_filled(
        egui::Rect::from_min_size(rect.min + vec2(4.0, 5.0), vec2(3.0, height - 10.0)),
        1.5,
        colour,
    );
    let mut row = ui.new_child(
        UiBuilder::new()
            .max_rect(rect.shrink2(vec2(14.0, 4.0)))
            .layout(Layout::top_down(Align::Min)),
    );
    row.spacing_mut().item_spacing.y = 0.0;
    row.horizontal(|ui| {
        ui.add_sized(
            vec2(64.0, 18.0),
            egui::Label::new(RichText::new(item.when(day)).weak()).halign(Align::Min),
        );
        ui.add(egui::Label::new(&item.title).truncate());
    });
    if !item.location.is_empty() {
        row.horizontal(|ui| {
            ui.add_space(64.0 + ui.spacing().item_spacing.x);
            ui.add(egui::Label::new(RichText::new(&item.location).small().weak()).truncate());
        });
    }
    response.on_hover_text(if item.calendar.is_empty() {
        "Open in Calendar".to_string()
    } else {
        format!("{}: open in Calendar", item.calendar)
    })
}

impl Shell for ClockMenu {
    fn setup(&mut self, ctx: &egui::Context) {
        self.skin.apply(ctx);
        if self.skin.follows_desktop() {
            patina::appearance::watch(ctx);
        }
    }

    fn ui(&mut self, ui: &mut Ui, _: &str) {
        if ui.input(|i| i.focused) {
            self.had_focus = true;
        } else if self.had_focus {
            self.done = true;
            return;
        }
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            self.done = true;
            return;
        }
        // Past midnight with the menu open.
        self.today = (self.clock)();
        self.ask();
        self.keys(ui);

        let rect = ui.max_rect();
        let v = ui.visuals().clone();
        ui.painter().rect(
            rect,
            patina_shell::hud::corner_radius(),
            v.panel_fill,
            v.window_stroke,
            StrokeKind::Inside,
        );
        let mut ui = ui.new_child(
            UiBuilder::new()
                .max_rect(rect.shrink(MARGIN))
                .layout(Layout::top_down(Align::Min)),
        );
        self.header(&mut ui);
        ui.add_space(8.0);
        self.month_bar(&mut ui);
        ui.add_space(4.0);
        self.month(&mut ui);
        ui.separator();
        self.events(&mut ui);
    }

    fn finished(&self) -> bool {
        self.done
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, m, d).unwrap()
    }

    fn menu() -> (ClockMenu, mpsc::Receiver<Request>) {
        let (send, receive) = mpsc::channel();
        (
            ClockMenu::new(day(10, 23), Skin::default_skin(), send),
            receive,
        )
    }

    #[test]
    fn a_months_grid_starts_on_the_monday_before_its_first() {
        // 1 October 2026 is a Thursday.
        assert_eq!(grid_start(day(10, 23)), day(9, 28));
        // 1 June 2026 is a Monday.
        assert_eq!(grid_start(day(6, 15)), day(6, 1));
    }

    #[test]
    fn each_months_events_are_asked_for_once() {
        let (mut m, requests) = menu();
        m.ask();
        assert_eq!(
            requests.try_recv(),
            Ok(Request::Agenda(day(9, 28), GRID_DAYS))
        );
        m.choose(day(10, 5));
        assert!(requests.try_recv().is_err());
        m.choose(day(11, 2));
        assert_eq!(
            requests.try_recv(),
            Ok(Request::Agenda(day(10, 26), GRID_DAYS))
        );
    }

    #[test]
    fn a_months_events_that_come_late_are_dropped() {
        let (mut m, _requests) = menu();
        m.ask();
        m.choose(day(11, 2));
        m.take_agenda(day(9, 28), Ok(Vec::new()));
        assert_eq!(m.agenda, None);
        m.take_agenda(day(10, 26), Err("Calendar isn't installed".into()));
        assert!(m.agenda.is_some());
        assert!(!m.calendar_here());
    }

    /// Draws the menu in each built-in skin, as PNGs in the folder
    /// PATINA_CLOCK_PREVIEW names, from made-up events on a made-up today.
    #[test]
    #[ignore]
    fn preview_the_menu() {
        let dir = std::path::PathBuf::from(std::env::var("PATINA_CLOCK_PREVIEW").unwrap());
        let (w, h) = (crate::SIZE.0 as usize, crate::SIZE.1 as usize);
        let at = |d: u32, time: &str| format!("2026-10-{d:02}T{time}+02:00");
        let item = |uid: &str,
                    title: &str,
                    location: &str,
                    color: Option<&str>,
                    start: String,
                    end: String,
                    all_day: bool| {
            Item {
                uid: uid.into(),
                title: title.into(),
                location: location.into(),
                calendar: "Personal".into(),
                color: color.map(String::from),
                start: chrono::DateTime::parse_from_rfc3339(&start).unwrap(),
                end: chrono::DateTime::parse_from_rfc3339(&end).unwrap(),
                all_day,
            }
        };
        let items = vec![
            item(
                "trip",
                "Trip to Gothenburg",
                "",
                Some("#FF2968"),
                at(22, "00:00:00"),
                at(25, "00:00:00"),
                true,
            ),
            item(
                "sun",
                "Sunrise",
                "",
                Some("#FF8C00"),
                at(23, "07:44:36"),
                at(23, "07:49:36"),
                false,
            ),
            item(
                "standup",
                "Standup",
                "Video call",
                Some("#0088FF"),
                at(23, "09:30:00"),
                at(23, "09:45:00"),
                false,
            ),
            item(
                "dinner",
                "Dinner with a rather long title that won't fit",
                "Ulriksdals slott, Slottsallén, Solna",
                None,
                at(23, "19:00:00"),
                at(23, "22:00:00"),
                false,
            ),
            item(
                "gym",
                "Gym",
                "",
                Some("#34C759"),
                at(27, "07:00:00"),
                at(27, "08:00:00"),
                false,
            ),
        ];
        let (skins, _) = patina::skin::load_all(patina::skin::BUILT_IN, &[]);
        for skin in skins {
            let name = skin.name.clone();
            let (send, _requests) = mpsc::channel();
            let mut m = ClockMenu::new(day(10, 23), skin, send);
            m.clock = || day(10, 23);
            m.ask();
            m.take_agenda(day(9, 28), Ok(items.clone()));
            let ctx = egui::Context::default();
            m.setup(&ctx);
            let mut painter = patina_shell::raster::Painter::default();
            let mut pixels = vec![0u32; w * h];
            for _ in 0..3 {
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        pos2(0.0, 0.0),
                        vec2(w as f32, h as f32),
                    )),
                    max_texture_side: Some(8192),
                    ..Default::default()
                };
                let out = ctx.run_ui(input, |ui| m.ui(ui, ""));
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
            let file = dir.join(format!("clock-{name}.png"));
            image::save_buffer(&file, &rgba, w as u32, h as u32, image::ColorType::Rgba8).unwrap();
        }
    }

    #[test]
    fn opening_an_event_runs_calendar_and_closes() {
        let (mut m, requests) = menu();
        m.open(day(10, 24), Some("gym"));
        assert_eq!(
            requests.try_recv(),
            Ok(Request::Run(
                "calendar --open 2026-10-24 --event 'gym'".into()
            ))
        );
        assert!(m.finished());
    }
}
