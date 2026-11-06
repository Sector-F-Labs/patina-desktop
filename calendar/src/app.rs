//! The window: Patina's frame with menus and the view controls in its
//! header bar, the sidebar (mini month and calendars), the day, week or
//! month view, and the event and account dialogs.

use std::collections::{HashMap, HashSet};
use std::time::Duration as StdDuration;

use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveTime};
use chrono_tz::Tz;
use egui::{Color32, Key, KeyboardShortcut, Modifiers, RichText};
use patina::frame::{self, TitleBar};
use patina::skin::{self, Skin};

use crate::caldav::Account;
use crate::config::{self, Config};
use crate::edit::{self, Draft};
use crate::model::{self, Calendar, Event, Occurrence};
use crate::sync::{self, Command, Snapshot, Stored, Sync, Update};
use crate::views::{self, Action, Scene, View};

/// Refresh from the server this often while the app is open.
const REFRESH_EVERY: f64 = 300.0;

/// Colours offered for a calendar, as in Apple's and GNOME's calendars.
const PALETTE: &[(&str, &str)] = &[
    ("Red", "#FF3B30"),
    ("Orange", "#FF9500"),
    ("Yellow", "#FFCC00"),
    ("Green", "#34C759"),
    ("Teal", "#30B0C7"),
    ("Blue", "#007AFF"),
    ("Purple", "#AF52DE"),
    ("Pink", "#FF2D55"),
    ("Brown", "#A2845E"),
    ("Graphite", "#8E8E93"),
];

enum Dialog {
    Event {
        draft: Draft,
        /// The event being edited, and the occurrence that was opened.
        existing: Option<(usize, DateTime<Tz>)>,
        error: Option<String>,
    },
    Delete {
        event: usize,
        occurrence: DateTime<Tz>,
    },
    Account {
        account: Account,
        error: Option<String>,
    },
}

/// Something asked for from the header bar or a menu, done once it's drawn.
enum Do {
    NewEvent,
    Refresh,
    Account,
    Quit,
    View(View),
    Today,
    Step(i32),
    SetSkin(String),
    SetLightDark(String),
    ReloadSkins,
    CopySkins,
    OpenSkinsFolder,
    SystemTitleBar(bool),
}

pub struct App {
    config: Config,
    account: Account,
    skins: Vec<Skin>,
    skin_errors: Vec<String>,
    forced_skin: Option<String>,
    sync: Sync,
    calendars: Vec<Calendar>,
    objects: Vec<Stored>,
    events: Vec<Event>,
    colors: HashMap<String, Color32>,
    local: Tz,
    focus: NaiveDate,
    mini_shown: NaiveDate,
    /// Scroll the time grid to this hour on the next frame.
    scroll_to: Option<f32>,
    dialog: Option<Dialog>,
    /// The latest message for the status bar.
    status: Option<Status>,
    synced_at: Option<DateTime<Tz>>,
    last_refresh: f64,
    /// For editing a custom colour in a calendar's menu.
    custom_color: Color32,
    /// Screenshot mode: where to save, whether asked, and when data arrived.
    screenshot: Option<(std::path::PathBuf, bool, Option<f64>)>,
    /// An event to open, by its day and UID, once it's among the events
    /// (the cache can be empty until the first sync).
    opening: Option<(NaiveDate, String)>,
}

/// Where the window opens, from the command line: a day, and an event's
/// occurrence on it, by UID.
#[derive(Debug, Default, PartialEq)]
pub struct Opening {
    pub day: Option<NaiveDate>,
    pub event: Option<String>,
}

pub fn local_zone() -> Tz {
    iana_time_zone::get_timezone().ok().and_then(|n| n.parse().ok()).unwrap_or(Tz::UTC)
}

/// The skin used until one is picked: the app's own, built into Patina.
const HOME_SKIN: &str = "Calendar";

/// The app's name for its config folder, and so its skins folder.
const APP: &str = "calendar";

fn load_skins() -> (Vec<Skin>, Vec<String>) {
    skin::load_all(skin::BUILT_IN, &skin::app_dir(APP).into_iter().collect::<Vec<_>>())
}

/// A message for the status bar.
struct Status {
    level: Level,
    text: String,
    /// From syncing, so the next sync that works clears it.
    sync: bool,
}

#[allow(dead_code)] // Nothing warns yet.
enum Level {
    Error,
    Warn,
    Info,
}

impl Status {
    fn error(text: impl Into<String>) -> Self {
        Status { level: Level::Error, text: text.into(), sync: false }
    }

    fn info(text: impl Into<String>) -> Self {
        Status { level: Level::Info, text: text.into(), sync: false }
    }
}

/// What Copy built-in skins did, for the status bar.
fn export_summary(copied: &[String], skipped: &[String]) -> String {
    let mut parts = Vec::new();
    if !copied.is_empty() {
        parts.push(format!("Copied {}", copied.join(", ")));
    }
    if !skipped.is_empty() {
        parts.push(format!("kept your existing {}", skipped.join(", ")));
    }
    parts.join("; ")
}


impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, opening: Opening) -> Self {
        patina::appearance::watch(&cc.egui_ctx);
        let mut config = Config::load();
        if let Some(view) = std::env::var("CALENDAR_VIEW").ok().and_then(|v| match v.as_str() {
            "day" => Some(View::Day),
            "week" => Some(View::Week),
            "month" => Some(View::Month),
            _ => None,
        }) {
            config.view = view;
        }
        let account = config::load_account();
        let cached = sync::load_cache();
        let local = local_zone();
        let today = chrono::Utc::now().with_timezone(&local).date_naive();
        let sync = Sync::start(&cc.egui_ctx, account.clone(), cached.clone());
        let (skins, skin_errors) = load_skins();
        let screenshot = std::env::var_os("CALENDAR_SCREENSHOT").map(|p| (std::path::PathBuf::from(p), false, None));
        let mut app = Self {
            config,
            account,
            skins,
            skin_errors,
            forced_skin: std::env::var("CALENDAR_SKIN").ok(),
            sync,
            calendars: Vec::new(),
            objects: Vec::new(),
            events: Vec::new(),
            colors: HashMap::new(),
            local,
            focus: today,
            mini_shown: today,
            scroll_to: Some(7.0),
            dialog: None,
            status: None,
            synced_at: None,
            last_refresh: 0.0,
            custom_color: Color32::from_rgb(0x00, 0x7A, 0xFF),
            screenshot,
            opening: opening.day.zip(opening.event),
        };
        if let Some(day) = opening.day {
            app.go_to(day);
        }
        app.adopt(cached);
        app.apply_skin(&cc.egui_ctx);
        if app.account.is_set() {
            app.sync.send(Command::Refresh);
        } else if app.screenshot.is_none() {
            app.dialog = Some(Dialog::Account {
                account: Account { url: "https://".into(), ..Default::default() },
                error: None,
            });
        }
        app
    }

    fn skin(&self) -> &Skin {
        let name = self.forced_skin.as_deref().or(self.config.skin.as_deref()).unwrap_or(HOME_SKIN);
        self.skins.iter().find(|s| s.name == name).unwrap_or(&self.skins[0])
    }

    fn today(&self) -> NaiveDate {
        self.now().date_naive()
    }

    fn now(&self) -> DateTime<Tz> {
        chrono::Utc::now().with_timezone(&self.local)
    }

    /// Takes in fetched data.
    fn adopt(&mut self, snapshot: Snapshot) {
        self.calendars = snapshot.calendars.iter().map(Calendar::from).collect();
        self.objects = snapshot.objects;
        self.rebuild();
        self.open_asked();
    }

    /// Opens the event the command line asked for, at its occurrence on
    /// the day it named, once it's there.
    fn open_asked(&mut self) {
        let Some((day, uid)) = &self.opening else { return };
        let found = self.occurrences(*day, *day).into_iter().find(|o| self.events[o.event].uid == *uid);
        if let Some(occ) = found {
            let draft = Draft::of(&self.events[occ.event], self.local);
            self.dialog = Some(Dialog::Event { draft, existing: Some((occ.event, occ.start)), error: None });
            self.opening = None;
        }
    }

    /// Re-reads the events and colours from the stored objects.
    fn rebuild(&mut self) {
        self.events = self
            .objects
            .iter()
            .flat_map(|o| model::events_from_ics(&o.calendar, &o.href, o.etag.clone(), &o.ics, self.local))
            .collect();
        self.colors = self
            .calendars
            .iter()
            .filter_map(|c| Some((c.href.clone(), views::parse_color(c.color.as_deref()?)?)))
            .collect();
    }

    fn visible(&self, calendar: &str) -> bool {
        !self.config.hidden.iter().any(|h| h == calendar)
    }

    fn occurrences(&self, from: NaiveDate, to: NaiveDate) -> Vec<Occurrence> {
        let hidden: HashSet<&str> = self.config.hidden.iter().map(String::as_str).collect();
        model::occurrences(&self.events, from, to, self.local, |c| !hidden.contains(c))
    }

    /// The days the current view shows.
    fn range(&self) -> (NaiveDate, NaiveDate) {
        match self.config.view {
            View::Day => (self.focus, self.focus),
            View::Week => {
                let s = views::week_start(self.focus);
                (s, s + Duration::days(6))
            }
            View::Month => {
                let s = views::month_grid(self.focus);
                (s, s + Duration::days(41))
            }
        }
    }

    fn title(&self) -> String {
        match self.config.view {
            View::Day => self.focus.format("%A %-d %B %Y").to_string(),
            View::Week => {
                let (a, b) = self.range();
                if a.month() == b.month() {
                    format!("{} – {}", a.format("%-d"), b.format("%-d %B %Y"))
                } else if a.year() == b.year() {
                    format!("{} – {}", a.format("%-d %b"), b.format("%-d %b %Y"))
                } else {
                    format!("{} – {}", a.format("%-d %b %Y"), b.format("%-d %b %Y"))
                }
            }
            View::Month => self.focus.format("%B %Y").to_string(),
        }
    }

    fn go_to(&mut self, day: NaiveDate) {
        self.focus = day;
        self.mini_shown = day;
        self.scroll_to = Some(if day == self.today() { (self.now().time().hour_f() - 1.5).max(0.0) } else { 7.0 });
    }

    fn step(&mut self, by: i32) {
        let day = match self.config.view {
            View::Day => self.focus + Duration::days(by as i64),
            View::Week => self.focus + Duration::weeks(by as i64),
            View::Month => {
                let first = self.focus.with_day(1).unwrap();
                let moved = if by > 0 { first + Duration::days(32) } else { first - Duration::days(1) };
                moved.with_day(1).unwrap().with_day(self.focus.day().min(28)).unwrap()
            }
        };
        self.go_to(day);
    }

    fn act(&mut self, ctx: &egui::Context, action: Do) {
        match action {
            Do::NewEvent => {
                let start = self.focus.and_time(NaiveTime::from_hms_opt(9, 0, 0).unwrap());
                let start =
                    chrono::TimeZone::from_local_datetime(&self.local, &start).earliest().unwrap_or_else(|| self.now());
                self.new_event(start, false);
            }
            Do::Refresh => self.sync.send(Command::Refresh),
            Do::Account => {
                let account = if self.account.is_set() {
                    self.account.clone()
                } else {
                    Account { url: "https://".into(), ..Default::default() }
                };
                self.dialog = Some(Dialog::Account { account, error: None });
            }
            Do::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Do::View(view) => {
                self.config.view = view;
                self.config.save();
                self.go_to(self.focus);
            }
            Do::Today => self.go_to(self.today()),
            Do::Step(by) => self.step(by),
            Do::SetSkin(name) => {
                self.forced_skin = None;
                self.config.skin = Some(name);
                self.config.save();
                self.apply_skin(ctx);
            }
            Do::SetLightDark(value) => {
                self.config.light_dark = value;
                self.config.save();
                self.apply_skin(ctx);
            }
            Do::ReloadSkins => {
                (self.skins, self.skin_errors) = load_skins();
                self.apply_skin(ctx);
            }
            Do::CopySkins => {
                let copied = skin::app_dir(APP)
                    .ok_or_else(|| "no config folder".to_string())
                    .and_then(|dir| skin::export_built_ins(&dir, skin::BUILT_IN));
                self.status = Some(match copied {
                    Ok(export) => {
                        // The copies share the built-ins' names, so after a reload they're the
                        // versions in use.
                        (self.skins, self.skin_errors) = load_skins();
                        self.apply_skin(ctx);
                        Status::info(export_summary(&export.copied, &export.skipped))
                    }
                    Err(e) => Status::error(format!("Couldn't copy skins: {e}")),
                });
            }
            Do::OpenSkinsFolder => {
                if let Some(dir) = skin::app_dir(APP) {
                    let _ = std::fs::create_dir_all(&dir);
                    let _ = patina::open::path(&dir);
                }
            }
            Do::SystemTitleBar(on) => {
                self.config.system_title_bar = on;
                self.config.save();
                ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(on));
            }
        }
    }

    fn new_event(&mut self, start: DateTime<Tz>, all_day: bool) {
        // In the first visible calendar, or the first one at all.
        let calendar =
            self.calendars.iter().find(|c| self.visible(&c.href)).or(self.calendars.first()).map(|c| c.href.clone());
        let Some(calendar) = calendar else {
            self.status = Some(Status::error("There's no calendar to add the event to yet"));
            return;
        };
        self.dialog = Some(Dialog::Event { draft: Draft::new(&calendar, start, all_day), existing: None, error: None });
    }

    // -----------------------------------------------------------------------
    // Saving and deleting, applied locally at once and sent to the server
    // -----------------------------------------------------------------------

    fn stored(&self, href: &str) -> Option<&Stored> {
        self.objects.iter().find(|o| o.href == href)
    }

    fn put_local(&mut self, calendar: &str, href: &str, ics: String, etag: Option<String>) {
        self.objects.retain(|o| o.href != href);
        self.objects.push(Stored { calendar: calendar.to_string(), href: href.to_string(), etag, ics });
        self.rebuild();
    }

    fn save_event(&mut self, draft: &Draft, existing: Option<usize>) -> Result<(), String> {
        if let Some(problem) = draft.problem(self.local) {
            return Err(problem.into());
        }
        match existing {
            None => {
                let (file, ics) = edit::create(draft, self.local);
                let href = format!("{}{file}", draft.calendar);
                self.put_local(&draft.calendar, &href, ics.clone(), None);
                self.sync.send(Command::Save { href, ics, etag: None, replaces: None });
            }
            Some(i) => {
                let ev = self.events[i].clone();
                let obj = self.stored(&ev.href).cloned().ok_or("The event's file is missing; refresh and try again")?;
                let ics =
                    edit::update(&obj.ics, ev.index, draft, self.local).ok_or("Couldn't find the event in its file")?;
                if draft.calendar == ev.calendar {
                    self.put_local(&ev.calendar, &ev.href, ics.clone(), obj.etag.clone());
                    self.sync.send(Command::Save { href: ev.href, ics, etag: obj.etag, replaces: None });
                } else {
                    let file = ev.href.rsplit('/').next().unwrap_or("event.ics").to_string();
                    let href = format!("{}{file}", draft.calendar);
                    self.objects.retain(|o| o.href != ev.href);
                    self.put_local(&draft.calendar, &href, ics.clone(), None);
                    self.sync.send(Command::Save { href, ics, etag: None, replaces: Some((ev.href, obj.etag)) });
                }
            }
        }
        Ok(())
    }

    /// Deletes an event, or with `only` set, just that occurrence of a series.
    fn delete_event(&mut self, i: usize, only: Option<DateTime<Tz>>) {
        let ev = self.events[i].clone();
        let Some(obj) = self.stored(&ev.href).cloned() else { return };
        let changed = match only {
            Some(occurrence) => edit::exclude(&obj.ics, ev.index, occurrence, self.local),
            None => edit::remove(&obj.ics, ev.index),
        };
        match changed {
            Some(ics) => {
                self.put_local(&ev.calendar, &ev.href, ics.clone(), obj.etag.clone());
                self.sync.send(Command::Save { href: ev.href, ics, etag: obj.etag, replaces: None });
            }
            None => {
                self.objects.retain(|o| o.href != ev.href);
                self.rebuild();
                self.sync.send(Command::Delete { href: ev.href, etag: obj.etag });
            }
        }
    }

    fn set_color(&mut self, calendar: &str, color: String) {
        if let Some(c) = self.calendars.iter_mut().find(|c| c.href == calendar) {
            c.color = Some(color.clone());
        }
        self.rebuild();
        self.sync.send(Command::SetColor { calendar: calendar.to_string(), color });
    }

    // -----------------------------------------------------------------------
    // Input
    // -----------------------------------------------------------------------

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let cmd = Modifiers::COMMAND;
        let mut actions = Vec::new();
        ctx.input_mut(|i| {
            let mut pressed = |mods, key| i.consume_shortcut(&KeyboardShortcut::new(mods, key));
            for (mods, key, action) in [
                (cmd, Key::N, Do::NewEvent),
                (cmd, Key::R, Do::Refresh),
                (cmd, Key::Q, Do::Quit),
                (cmd, Key::Num1, Do::View(View::Day)),
                (cmd, Key::Num2, Do::View(View::Week)),
                (cmd, Key::Num3, Do::View(View::Month)),
                (cmd, Key::T, Do::Today),
                (Modifiers::NONE, Key::PageUp, Do::Step(-1)),
                (Modifiers::NONE, Key::PageDown, Do::Step(1)),
            ] {
                if pressed(mods, key) {
                    actions.push(action);
                }
            }
        });
        // Plain keys, when nothing is being typed into: as in GNOME Calendar.
        if !ctx.egui_wants_keyboard_input() {
            ctx.input_mut(|i| {
                let mut pressed = |key| i.consume_key(Modifiers::NONE, key);
                for (key, action) in [
                    (Key::ArrowLeft, Do::Step(-1)),
                    (Key::ArrowRight, Do::Step(1)),
                    (Key::T, Do::Today),
                    (Key::D, Do::View(View::Day)),
                    (Key::W, Do::View(View::Week)),
                    (Key::M, Do::View(View::Month)),
                ] {
                    if pressed(key) {
                        actions.push(action);
                    }
                }
            });
        }
        for a in actions {
            self.act(ctx, a);
        }
    }

    // -----------------------------------------------------------------------
    // UI
    // -----------------------------------------------------------------------

    fn header(&mut self, ui: &mut egui::Ui, style: &frame::FrameStyle) {
        let title = self.title();
        let custom = !self.config.system_title_bar;
        let mut left = Vec::new();
        let mut right = Vec::new();
        let view = self.config.view;
        TitleBar::new(&title, style).custom(custom).menu_bar(true).span(frame::header_span(ui)).show(
            ui,
            |ui| self.menus(ui, &mut left),
            |ui| {
                for (v, label, keys) in
                    [(View::Month, "Month", "M"), (View::Week, "Week", "W"), (View::Day, "Day", "D")]
                {
                    if ui.selectable_label(view == v, label).on_hover_text(format!("{label} view ({keys})")).clicked() {
                        right.push(Do::View(v));
                    }
                }
                ui.separator();
                if ui.button("›").on_hover_text("Next (→)").clicked() {
                    right.push(Do::Step(1));
                }
                if ui.button("Today").on_hover_text("Go to today (T)").clicked() {
                    right.push(Do::Today);
                }
                if ui.button("‹").on_hover_text("Previous (←)").clicked() {
                    right.push(Do::Step(-1));
                }
            },
        );
        for a in left.into_iter().chain(right) {
            self.act(ui.ctx(), a);
        }
    }

    fn menus(&self, ui: &mut egui::Ui, actions: &mut Vec<Do>) {
        fn item(ui: &mut egui::Ui, actions: &mut Vec<Do>, enabled: bool, label: &str, keys: &str, action: Do) {
            if ui.add_enabled(enabled, egui::Button::new(label).shortcut_text(keys)).clicked() {
                actions.push(action);
                ui.close();
            }
        }
        let connected = self.account.is_set();
        ui.menu_button("File", |ui| {
            item(ui, actions, !self.calendars.is_empty(), "New Event…", "Ctrl+N", Do::NewEvent);
            item(ui, actions, connected, "Refresh", "Ctrl+R", Do::Refresh);
            ui.separator();
            item(ui, actions, true, "Account…", "", Do::Account);
            ui.separator();
            item(ui, actions, true, "Quit", "Ctrl+Q", Do::Quit);
        });
        ui.menu_button("View", |ui| {
            item(ui, actions, true, "Day", "Ctrl+1", Do::View(View::Day));
            item(ui, actions, true, "Week", "Ctrl+2", Do::View(View::Week));
            item(ui, actions, true, "Month", "Ctrl+3", Do::View(View::Month));
            ui.separator();
            item(ui, actions, true, "Today", "Ctrl+T", Do::Today);
            item(ui, actions, true, "Previous", "PgUp", Do::Step(-1));
            item(ui, actions, true, "Next", "PgDn", Do::Step(1));
            ui.separator();
            let mut system = self.config.system_title_bar;
            if ui.checkbox(&mut system, "System title bar").changed() {
                actions.push(Do::SystemTitleBar(system));
            }
        });
        // Patina's skin menu (its ADR-0017): only controls; what they did goes to the status bar.
        ui.menu_button("Skins", |ui| {
            // Size the menu to its widest entry instead of wrapping entries to its first width.
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
            let current = self.skin();
            for skin in &self.skins {
                let label = match &skin.author {
                    Some(author) if skin.path.is_some() => format!("{} — {author}", skin.name),
                    _ => skin.name.clone(),
                };
                let resp = ui.radio(skin.name == current.name, label);
                let resp = match &skin.path {
                    Some(path) => resp.on_hover_text(path.display().to_string()),
                    None => resp,
                };
                if resp.clicked() {
                    actions.push(Do::SetSkin(skin.name.clone()));
                }
            }
            ui.separator();
            ui.add_enabled_ui(current.follows_desktop(), |ui| {
                for (value, label) in [("system", "Follow desktop"), ("light", "Light"), ("dark", "Dark")] {
                    // An unset or unknown setting follows the desktop.
                    let on = match value {
                        "system" => !matches!(self.config.light_dark.as_str(), "light" | "dark"),
                        _ => self.config.light_dark == value,
                    };
                    if ui.radio(on, label).clicked() {
                        actions.push(Do::SetLightDark(value.into()));
                    }
                }
            });
            ui.separator();
            if ui.button("Reload skins").clicked() {
                actions.push(Do::ReloadSkins);
            }
            if ui
                .button("Copy built-in skins to folder")
                .on_hover_text("Puts editable copies in the skins folder; never overwrites existing files")
                .clicked()
            {
                actions.push(Do::CopySkins);
            }
            let folder = skin::app_dir(APP).map(|d| d.display().to_string()).unwrap_or_default();
            if ui.button("Open skins folder").on_hover_text(folder).clicked() {
                actions.push(Do::OpenSkinsFolder);
            }
        });
    }

    fn apply_skin(&self, ctx: &egui::Context) {
        let skin = self.skin();
        skin.apply(ctx);
        if skin.follows_desktop() {
            ctx.set_theme(self.config.theme());
        }
    }

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(6.0);
        let (from, _) = (views::month_grid(self.mini_shown), ());
        let busy: HashSet<NaiveDate> = self
            .occurrences(from, from + Duration::days(41))
            .iter()
            .flat_map(|o| {
                let (a, b) = o.days();
                (0..=(b - a).num_days().min(41)).map(move |k| a + Duration::days(k))
            })
            .collect();
        let today = self.today();
        if let Some(day) = views::mini_month(ui, &mut self.mini_shown, self.focus, today, &|d| busy.contains(&d)) {
            self.go_to(day);
        }
        ui.add_space(10.0);
        ui.separator();
        ui.label(RichText::new("Calendars").strong());
        ui.add_space(2.0);
        let calendars = self.calendars.clone();
        let mut toggled = None;
        let mut recolor = None;
        egui::ScrollArea::vertical().auto_shrink([false, true]).max_height(ui.available_height() - 36.0).show(
            ui,
            |ui| {
                for cal in &calendars {
                    let color = self.colors.get(&cal.href).copied().unwrap_or(ui.visuals().selection.bg_fill);
                    let on = self.visible(&cal.href);
                    let resp = calendar_row(ui, &cal.name, color, on);
                    if resp.clicked() {
                        toggled = Some(cal.href.clone());
                    }
                    let resp = resp.on_hover_text("Click to show or hide; right-click for its colour");
                    patina::menu::context(&resp, |ui| {
                        ui.label(RichText::new(&cal.name).strong());
                        ui.separator();
                        for (name, hex) in PALETTE {
                            let swatch = views::parse_color(hex).unwrap();
                            let picked = ui
                                .horizontal(|ui| {
                                    let (r, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                                    ui.painter().rect_filled(r, 3.0, swatch);
                                    ui.selectable_label(
                                        cal.color.as_deref().is_some_and(|c| c.eq_ignore_ascii_case(hex)),
                                        *name,
                                    )
                                })
                                .inner;
                            if picked.clicked() {
                                recolor = Some((cal.href.clone(), hex.to_string()));
                                ui.close();
                            }
                        }
                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.color_edit_button_srgba(&mut self.custom_color);
                            if ui.button("Use custom colour").clicked() {
                                let c = self.custom_color;
                                recolor = Some((cal.href.clone(), format!("#{:02X}{:02X}{:02X}", c.r(), c.g(), c.b())));
                                ui.close();
                            }
                        });
                    });
                }
            },
        );
        if let Some(href) = toggled {
            if self.visible(&href) {
                self.config.hidden.push(href);
            } else {
                self.config.hidden.retain(|h| h != &href);
            }
            self.config.save();
        }
        if let Some((href, color)) = recolor {
            self.set_color(&href, color);
        }
    }

    /// Patina's status bar (its ADR-0016): the version, then the latest message coloured by its
    /// level; on the right, the connection to the server.
    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            // Just the version: the app's name is already in the title bar and the window list,
            // so repeating it here adds nothing.
            ui.label(RichText::new(format!("v{}", env!("CARGO_PKG_VERSION"))).weak().small());
            // Skins that failed to load stay until they load, ahead of any newer message.
            let skin_errors = (!self.skin_errors.is_empty()).then(|| {
                let n = self.skin_errors.len();
                Status::error(format!("Couldn't load {n} skin{}", if n == 1 { "" } else { "s" }))
            });
            if let Some(status) = skin_errors.as_ref().or(self.status.as_ref()) {
                ui.separator();
                let color = match status.level {
                    Level::Error => ui.visuals().error_fg_color,
                    Level::Warn => ui.visuals().warn_fg_color,
                    Level::Info => ui.visuals().weak_text_color(),
                };
                let resp = ui.label(RichText::new(&status.text).color(color).small());
                if skin_errors.is_some() {
                    resp.on_hover_text(self.skin_errors.join("\n"));
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.sync.busy {
                    ui.label(RichText::new("Syncing…").weak().small());
                    ui.spinner();
                } else if !self.account.is_set() {
                    if ui.small_button("Set up account…").clicked() {
                        self.act(ui.ctx(), Do::Account);
                    }
                } else if let Some(at) = self.synced_at {
                    ui.label(RichText::new(format!("Updated {}", at.format("%H:%M"))).weak().small())
                        .on_hover_text(self.account.url.clone());
                }
            });
        });
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.dialog.take() else { return };
        let mut keep = true;
        let calendars = self.calendars.clone();
        let colors = self.colors.clone();
        let events_len = self.events.len();
        match &mut dialog {
            Dialog::Event { draft, existing, error } => {
                let recurring = existing.and_then(|(i, _)| self.events.get(i)).is_some_and(|e| e.recurring());
                let mut save = false;
                let mut delete = false;
                let modal = egui::Modal::new(egui::Id::new("event-dialog")).show(ctx, |ui| {
                    ui.set_width(380.0);
                    ui.heading(if existing.is_some() { "Event" } else { "New Event" });
                    ui.add_space(6.0);
                    let title = ui.add(
                        egui::TextEdit::singleline(&mut draft.summary).hint_text("Title").desired_width(f32::INFINITY),
                    );
                    if existing.is_none() && !title.has_focus() && ui.memory(|m| m.focused().is_none()) {
                        title.request_focus();
                    }
                    egui::Grid::new("event-fields").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                        ui.label("Calendar");
                        let current = calendars
                            .iter()
                            .find(|c| c.href == draft.calendar)
                            .map(|c| c.name.clone())
                            .unwrap_or_default();
                        egui::ComboBox::from_id_salt("event-calendar").selected_text(current).show_ui(ui, |ui| {
                            for c in &calendars {
                                let color = colors.get(&c.href).copied().unwrap_or(ui.visuals().selection.bg_fill);
                                ui.horizontal(|ui| {
                                    let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                                    ui.painter().circle_filled(r.center(), 5.0, color);
                                    ui.selectable_value(&mut draft.calendar, c.href.clone(), &c.name);
                                });
                            }
                        });
                        ui.end_row();
                        ui.label("");
                        ui.checkbox(&mut draft.all_day, "All day");
                        ui.end_row();
                        ui.label("Starts");
                        ui.horizontal(|ui| {
                            let before = draft.start_date;
                            date_field(ui, &mut draft.start_date, "start-date");
                            if draft.start_date != before {
                                // Keep the length when moving the start.
                                draft.end_date += draft.start_date - before;
                            }
                            if !draft.all_day {
                                time_field(ui, &mut draft.start_time, "start-time");
                            }
                        });
                        ui.end_row();
                        ui.label("Ends");
                        ui.horizontal(|ui| {
                            date_field(ui, &mut draft.end_date, "end-date");
                            if !draft.all_day {
                                time_field(ui, &mut draft.end_time, "end-time");
                            }
                        });
                        ui.end_row();
                        ui.label("Location");
                        ui.add(egui::TextEdit::singleline(&mut draft.location).desired_width(f32::INFINITY));
                        ui.end_row();
                        ui.label("Notes");
                        ui.add(
                            egui::TextEdit::multiline(&mut draft.description)
                                .desired_rows(3)
                                .desired_width(f32::INFINITY),
                        );
                        ui.end_row();
                    });
                    if recurring {
                        ui.label(
                            RichText::new("This event repeats. Changes apply to every occurrence.").weak().small(),
                        );
                    }
                    if let Some(e) = error {
                        ui.colored_label(ui.visuals().error_fg_color, e.as_str());
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        let enter = title.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                        if ui.button("Save").clicked() || enter {
                            save = true;
                        }
                        if ui.button("Cancel").clicked() {
                            keep = false;
                        }
                        if existing.is_some() {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.button(RichText::new("Delete").color(ui.visuals().error_fg_color)).clicked() {
                                    delete = true;
                                }
                            });
                        }
                    });
                });
                if modal.should_close() {
                    keep = false;
                }
                if save {
                    match self.save_event(draft, existing.map(|e| e.0)) {
                        Ok(()) => keep = false,
                        Err(e) => *error = Some(e),
                    }
                }
                if delete && let Some((i, occ)) = *existing {
                    if recurring {
                        dialog = Dialog::Delete { event: i, occurrence: occ };
                    } else {
                        self.delete_event(i, None);
                        keep = false;
                    }
                }
            }
            Dialog::Delete { event, occurrence } => {
                let (event, occurrence) = (*event, *occurrence);
                let mut choice = None;
                let modal = egui::Modal::new(egui::Id::new("delete-dialog")).show(ctx, |ui| {
                    ui.set_width(340.0);
                    ui.heading("Delete a repeating event");
                    ui.label(format!(
                        "Delete only the occurrence on {}, or every occurrence?",
                        occurrence.format("%A %-d %B")
                    ));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("This occurrence").clicked() {
                            choice = Some(Some(occurrence));
                        }
                        if ui.button(RichText::new("All occurrences").color(ui.visuals().error_fg_color)).clicked() {
                            choice = Some(None);
                        }
                        if ui.button("Cancel").clicked() {
                            keep = false;
                        }
                    });
                });
                if modal.should_close() {
                    keep = false;
                }
                if let Some(only) = choice
                    && event < events_len
                {
                    self.delete_event(event, only);
                    keep = false;
                }
            }
            Dialog::Account { account, error } => {
                let mut save = false;
                let modal = egui::Modal::new(egui::Id::new("account-dialog")).show(ctx, |ui| {
                    ui.set_width(380.0);
                    ui.heading("Account");
                    ui.label(RichText::new("The CalDAV server to show, such as plan on urd.").weak());
                    ui.add_space(6.0);
                    egui::Grid::new("account-fields").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                        ui.label("Server");
                        ui.add(
                            egui::TextEdit::singleline(&mut account.url)
                                .hint_text("https://plan.example.com/")
                                .desired_width(260.0),
                        );
                        ui.end_row();
                        ui.label("Username");
                        ui.add(egui::TextEdit::singleline(&mut account.username).desired_width(260.0));
                        ui.end_row();
                        ui.label("Password");
                        ui.add(egui::TextEdit::singleline(&mut account.password).password(true).desired_width(260.0));
                        ui.end_row();
                    });
                    ui.label(
                        RichText::new("Saved in ~/.config/calendar/account.toml, readable only by you.").weak().small(),
                    );
                    if let Some(e) = error {
                        ui.colored_label(ui.visuals().error_fg_color, e.as_str());
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.button("Connect").clicked() {
                            save = true;
                        }
                        if ui.button("Cancel").clicked() {
                            keep = false;
                        }
                    });
                });
                if modal.should_close() {
                    keep = false;
                }
                if save {
                    match crate::caldav::Client::new(account.clone()).and_then(|_| config::save_account(account)) {
                        Ok(()) => {
                            self.account = account.clone();
                            self.config.hidden.clear();
                            self.sync.send(Command::SetAccount(account.clone()));
                            keep = false;
                        }
                        Err(e) => *error = Some(e),
                    }
                }
            }
        }
        if keep && self.dialog.is_none() {
            self.dialog = Some(dialog);
        }
    }

    fn take_screenshot(&mut self, ctx: &egui::Context) {
        let Some((path, requested, synced)) = &mut self.screenshot else { return };
        ctx.request_repaint();
        let now = ctx.input(|i| i.time);
        if synced.is_none() && (!self.events.is_empty() || now > 6.0) {
            *synced = Some(now);
        }
        let image = ctx.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = image {
            let [w, h] = image.size;
            let saved = image::RgbaImage::from_raw(w as u32, h as u32, image.as_raw().to_vec())
                .ok_or_else(|| "screenshot has the wrong size".to_string())
                .and_then(|b| b.save(&*path).map_err(|e| e.to_string()));
            match saved {
                Ok(()) => eprintln!("saved {}", path.display()),
                Err(e) => eprintln!("screenshot failed: {e}"),
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else if !*requested && synced.is_some_and(|t| now - t > 0.8) {
            *requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
    }
}

/// A calendar in the sidebar: a checkbox in its colour, and its name.
fn calendar_row(ui: &mut egui::Ui, name: &str, color: Color32, on: bool) -> egui::Response {
    let height = 24.0;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), height), egui::Sense::click());
    let v = ui.visuals();
    if resp.hovered() {
        ui.painter().rect_filled(rect, 4.0, v.widgets.hovered.weak_bg_fill);
    }
    let boxed = egui::Rect::from_center_size(egui::pos2(rect.left() + 14.0, rect.center().y), egui::vec2(14.0, 14.0));
    if on {
        ui.painter().rect_filled(boxed, 3.0, color);
        let c = boxed.center();
        let l = color.r() as f32 * 0.299 + color.g() as f32 * 0.587 + color.b() as f32 * 0.114;
        let tick = if l > 160.0 { Color32::from_gray(20) } else { Color32::WHITE };
        ui.painter().add(egui::Shape::line(
            vec![c + egui::vec2(-3.5, 0.0), c + egui::vec2(-1.0, 3.0), c + egui::vec2(4.0, -3.0)],
            egui::Stroke::new(1.8, tick),
        ));
    } else {
        ui.painter().rect_stroke(boxed, 3.0, egui::Stroke::new(1.5, color), egui::StrokeKind::Inside);
    }
    let fg = if on { v.text_color() } else { v.weak_text_color() };
    ui.painter().text(
        egui::pos2(rect.left() + 30.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        name,
        egui::FontId::proportional(13.5),
        fg,
    );
    resp
}

/// A date, with egui_extras' date picker (which works in jiff dates).
fn date_field(ui: &mut egui::Ui, date: &mut NaiveDate, id: &str) {
    let mut d = jiff::civil::date(date.year() as i16, date.month() as i8, date.day() as i8);
    ui.add(egui_extras::DatePickerButton::new(&mut d).id_salt(id).calendar_week(false).start_end_years(2000..=2100));
    if let Some(back) = NaiveDate::from_ymd_opt(d.year() as i32, d.month() as u32, d.day() as u32) {
        *date = back;
    }
}

/// Hours and minutes.
fn time_field(ui: &mut egui::Ui, time: &mut NaiveTime, id: &str) {
    use chrono::Timelike;
    let mut h = time.hour();
    let mut m = time.minute();
    ui.push_id(id, |ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        ui.add(egui::DragValue::new(&mut h).range(0..=23).custom_formatter(|v, _| format!("{v:02}")));
        ui.label(":");
        ui.add(egui::DragValue::new(&mut m).range(0..=59).speed(0.5).custom_formatter(|v, _| format!("{v:02}")));
    });
    if let Some(t) = NaiveTime::from_hms_opt(h, m, 0) {
        *time = t;
    }
}

trait HourF {
    fn hour_f(&self) -> f32;
}

impl HourF for NaiveTime {
    fn hour_f(&self) -> f32 {
        use chrono::Timelike;
        self.num_seconds_from_midnight() as f32 / 3600.0
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        for update in self.sync.poll() {
            match update {
                Update::Synced(snapshot) => {
                    self.adopt(snapshot);
                    self.synced_at = Some(self.now());
                    // A sync that works clears what the last one said went wrong.
                    if self.status.as_ref().is_some_and(|s| s.sync) {
                        self.status = None;
                    }
                }
                Update::Error(e) => self.status = Some(Status { sync: true, ..Status::error(e) }),
                Update::Busy(_) => {}
            }
        }
        let now = ctx.input(|i| i.time);
        if self.account.is_set() && !self.sync.busy && now - self.last_refresh > REFRESH_EVERY {
            if self.last_refresh > 0.0 {
                self.sync.send(Command::Refresh);
            }
            self.last_refresh = now;
        }
        if self.dialog.is_none() {
            self.shortcuts(ctx);
        }
        // Keep the current-time line moving.
        ctx.request_repaint_after(StdDuration::from_secs(30));
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        if !self.config.system_title_bar {
            patina::frame::shadow::request(frame);
        }
        let ctx = ui.ctx().clone();
        let custom = !self.config.system_title_bar;
        let style = self.skin().frame_style(ui.visuals());
        let radius = if custom { frame::corner_radius(&ctx, style.radius) } else { 0 };

        egui::Panel::top("header")
            .frame(frame::header_frame(ui.style(), &style, radius))
            .show(ui, |ui| self.header(ui, &style));

        // It spans the window, so it comes before the sidebar, and holds both bottom corners.
        egui::Panel::bottom("status")
            .frame(
                egui::Frame::side_top_panel(ui.style())
                    .inner_margin(egui::Margin::symmetric(10, 4))
                    .corner_radius(egui::CornerRadius { sw: radius, se: radius, ..Default::default() }),
            )
            .show(ui, |ui| self.status_bar(ui));

        egui::Panel::left("sidebar")
            .frame(egui::Frame::side_top_panel(ui.style()))
            .default_size(230.0)
            .resizable(true)
            .show(ui, |ui| self.sidebar(ui));

        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(ui.style()))
            .show(ui, |ui| {
                let (from, to) = self.range();
                let occurrences = self.occurrences(from, to);
                let scene = Scene {
                    events: &self.events,
                    occurrences: &occurrences,
                    colors: &self.colors,
                    local: self.local,
                    today: self.today(),
                    now: self.now(),
                };
                let action = match self.config.view {
                    View::Month => views::month(ui, &scene, self.focus),
                    View::Week => views::time_grid(ui, &scene, from, 7, &mut self.scroll_to),
                    View::Day => views::time_grid(ui, &scene, from, 1, &mut self.scroll_to),
                };
                match action {
                    Some(Action::Open(i, occ)) => {
                        let draft = Draft::of(&self.events[i], self.local);
                        self.dialog = Some(Dialog::Event { draft, existing: Some((i, occ)), error: None });
                    }
                    Some(Action::New { start, all_day }) => self.new_event(start, all_day),
                    Some(Action::ShowDay(day)) => {
                        self.config.view = View::Day;
                        self.config.save();
                        self.go_to(day);
                    }
                    None => {}
                }
            });

        self.dialogs(&ctx);
        if custom {
            frame::edges(&ctx, radius, style.border);
        }
        self.take_screenshot(&ctx);
    }

    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        frame::clear_color(visuals, !self.config.system_title_bar)
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        frame::release_grab(ctx, raw_input);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_home_skin_is_built_into_patina() {
        let (skins, errors) = skin::load_all(skin::BUILT_IN, &[]);
        assert!(errors.is_empty(), "{errors:?}");
        assert!(skins.iter().any(|s| s.name == HOME_SKIN));
    }
}
