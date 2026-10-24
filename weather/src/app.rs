//! The window: Patina's frame with menus and the view switch in its header
//! bar, the sidebar of places with search, the overview, week or day view,
//! and the status bar.

use std::collections::{HashMap, HashSet};
use std::time::Duration as StdDuration;

use chrono::{Duration, NaiveDate};
use egui::{Key, KeyboardShortcut, Modifiers, RichText};
use patina::frame::{self, TitleBar};
use patina::skin::{self, Skin};

use crate::config::{Config, View};
use crate::fetch::{self, Fetcher, Reply};
use crate::forecast::{self, Forecast, Place};
use crate::palette::{self, Palette};
use crate::units::{Length, Speed, Temperature};
use crate::widgets::{Action, Scene};
use crate::{day, icons, overview, week};

/// Fetch a place's forecast again once it's this old.
const STALE_AFTER: i64 = 15 * 60;

/// The skin used until one is picked: stock egui, following the desktop.
const HOME_SKIN: &str = skin::DEFAULT_SKIN;

/// The app's name for its config folder, and so its skins folder.
const APP: &str = "weather";

fn load_skins() -> (Vec<Skin>, Vec<String>) {
    skin::load_all(skin::BUILT_IN, &skin::app_dir(APP).into_iter().collect::<Vec<_>>())
}

/// A message for the status bar.
struct Status {
    level: Level,
    text: String,
}

enum Level {
    Error,
    Warn,
    Info,
}

impl Status {
    fn error(text: impl Into<String>) -> Self {
        Status { level: Level::Error, text: text.into() }
    }

    fn info(text: impl Into<String>) -> Self {
        Status { level: Level::Info, text: text.into() }
    }
}

/// Something asked for from the header bar, a menu or a key, done once it's drawn.
enum Do {
    Refresh,
    Search,
    Quit,
    View(View),
    Step(i64),
    ShowDay(NaiveDate),
    Days(usize),
    Select(usize),
    Remove(usize),
    Move(usize, isize),
    Add(Place),
    Units(crate::units::Units),
    SetSkin(String),
    SetLightDark(String),
    ReloadSkins,
    CopySkins,
    OpenSkinsFolder,
    SystemTitleBar(bool),
}

pub struct App {
    config: Config,
    skins: Vec<Skin>,
    skin_errors: Vec<String>,
    forced_skin: Option<String>,
    palette: Palette,
    /// Whether `palette` was made for a dark background.
    palette_dark: bool,
    fetcher: Fetcher,
    forecasts: HashMap<String, Forecast>,
    loading: HashSet<String>,
    /// The day the day view shows.
    day: Option<NaiveDate>,
    search: String,
    /// The query the results are for, and them.
    results: Option<(String, Result<Vec<Place>, String>)>,
    searching: Option<String>,
    focus_search: bool,
    status: Option<Status>,
    /// Screenshot mode: where to save, whether asked, and when data arrived.
    screenshot: Option<(std::path::PathBuf, bool, Option<f64>)>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        patina::appearance::watch(&cc.egui_ctx);
        let mut config = Config::load();
        if let Some(view) = std::env::var("WEATHER_VIEW").ok().and_then(|v| match v.as_str() {
            "overview" => Some(View::Overview),
            "week" => Some(View::Week),
            "day" => Some(View::Day),
            _ => None,
        }) {
            config.view = view;
        }
        let (skins, skin_errors) = load_skins();
        let screenshot = std::env::var_os("WEATHER_SCREENSHOT").map(|p| (std::path::PathBuf::from(p), false, None));
        let day = std::env::var("WEATHER_DAY")
            .ok()
            .and_then(|d| d.parse::<i64>().ok())
            .map(|k| chrono::Local::now().date_naive() + Duration::days(k));
        let mut app = Self {
            config,
            skins,
            skin_errors,
            forced_skin: std::env::var("WEATHER_SKIN").ok(),
            palette: Palette::new(false),
            palette_dark: false,
            fetcher: Fetcher::new(&cc.egui_ctx),
            forecasts: fetch::load_cache(),
            loading: HashSet::new(),
            day,
            search: String::new(),
            results: None,
            searching: None,
            focus_search: false,
            status: None,
            screenshot,
        };
        app.apply_skin(&cc.egui_ctx);
        if app.config.places.is_empty() {
            app.focus_search = true;
        }
        app.refresh_stale();
        app
    }

    fn skin(&self) -> &Skin {
        let find = |name: &str| self.skins.iter().find(|s| s.name == name);
        let name = self.forced_skin.as_deref().or(self.config.skin.as_deref()).unwrap_or(HOME_SKIN);
        find(name).or_else(|| find(HOME_SKIN)).unwrap_or(&self.skins[0])
    }

    fn apply_skin(&mut self, ctx: &egui::Context) {
        let skin = self.skin().clone();
        skin.apply(ctx);
        if skin.follows_desktop() {
            ctx.set_theme(self.config.theme());
        }
        self.update_palette(ctx);
    }

    /// The app's colours: the defaults for the current light or dark, with
    /// the skin's `[app.weather]` over them.
    fn update_palette(&mut self, ctx: &egui::Context) {
        let dark = ctx.global_style().visuals.dark_mode;
        self.palette_dark = dark;
        self.palette = match self.skin().app_section::<palette::Section>(APP) {
            Ok((section, warnings)) => {
                if let Some(w) = warnings.first() {
                    self.status = Some(Status { level: Level::Warn, text: format!("Skin: {w}") });
                }
                Palette::new(dark).with(section)
            }
            Err(e) => {
                self.status = Some(Status::error(format!("Skin: {e}")));
                Palette::new(dark)
            }
        };
    }

    fn place(&self) -> Option<&Place> {
        self.config.place()
    }

    fn forecast(&self) -> Option<&Forecast> {
        self.place().and_then(|p| self.forecasts.get(&p.key()))
    }

    fn fetch(&mut self, place: &Place) {
        if self.loading.insert(place.key()) {
            self.fetcher.forecast(place);
        }
    }

    /// Fetches every place whose forecast is missing or old.
    fn refresh_stale(&mut self) {
        let now = chrono::Utc::now();
        let stale: Vec<Place> = self
            .config
            .places
            .iter()
            .filter(|p| self.forecasts.get(&p.key()).is_none_or(|f| (now - f.fetched).num_seconds() > STALE_AFTER))
            .cloned()
            .collect();
        for p in stale {
            self.fetch(&p);
        }
    }

    /// The day the day view shows, kept within the forecast.
    fn shown_day(&self) -> Option<NaiveDate> {
        let f = self.forecast()?;
        let (first, last) = (f.days.first()?.date, f.days.last()?.date);
        Some(self.day.unwrap_or(f.today()).clamp(first, last))
    }

    fn title(&self) -> String {
        let Some(place) = self.place() else { return "Weather".into() };
        match (self.config.view, self.shown_day()) {
            (View::Day, Some(d)) => format!("{} · {}", place.name, d.format("%A %-d %B")),
            (View::Week, _) => format!("{} · {} days", place.name, self.config.days),
            _ => place.name.clone(),
        }
    }

    fn act(&mut self, ctx: &egui::Context, action: Do) {
        match action {
            Do::Refresh => {
                for p in self.config.places.clone() {
                    self.fetch(&p);
                }
            }
            Do::Search => self.focus_search = true,
            Do::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Do::View(view) => {
                self.config.view = view;
                self.config.save();
            }
            Do::Step(by) => {
                if let Some(d) = self.shown_day() {
                    self.day = Some(d + Duration::days(by));
                    self.day = self.shown_day();
                }
                self.config.view = View::Day;
            }
            Do::ShowDay(d) => {
                self.day = Some(d);
                self.config.view = View::Day;
                self.config.save();
            }
            Do::Days(n) => {
                self.config.days = n;
                self.config.save();
            }
            Do::Select(i) => {
                self.config.selected = i;
                self.config.save();
                self.day = None;
            }
            Do::Remove(i) => {
                if i < self.config.places.len() {
                    let gone = self.config.places.remove(i);
                    self.forecasts.remove(&gone.key());
                    fetch::save_cache(&self.forecasts);
                    if self.config.selected >= i && self.config.selected > 0 {
                        self.config.selected -= 1;
                    }
                    self.config.save();
                    self.status = Some(Status::info(format!("Removed {}", gone.name)));
                }
            }
            Do::Move(i, by) => {
                let j = i as isize + by;
                if j >= 0 && (j as usize) < self.config.places.len() {
                    self.config.places.swap(i, j as usize);
                    if self.config.selected == i {
                        self.config.selected = j as usize;
                    } else if self.config.selected == j as usize {
                        self.config.selected = i;
                    }
                    self.config.save();
                }
            }
            Do::Add(place) => {
                let i = match self.config.places.iter().position(|p| p.key() == place.key()) {
                    Some(i) => i,
                    None => {
                        self.config.places.push(place.clone());
                        self.fetch(&place);
                        self.config.places.len() - 1
                    }
                };
                self.config.selected = i;
                self.config.save();
                self.search.clear();
                self.results = None;
                self.day = None;
            }
            Do::Units(units) => {
                self.config.units = units;
                self.config.save();
            }
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

    // -----------------------------------------------------------------------
    // Input
    // -----------------------------------------------------------------------

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let cmd = Modifiers::COMMAND;
        let mut actions = Vec::new();
        ctx.input_mut(|i| {
            let mut pressed = |mods, key| i.consume_shortcut(&KeyboardShortcut::new(mods, key));
            for (mods, key, action) in [
                (cmd, Key::R, Do::Refresh),
                (cmd, Key::F, Do::Search),
                (cmd, Key::L, Do::Search),
                (cmd, Key::Q, Do::Quit),
                (cmd, Key::Num1, Do::View(View::Overview)),
                (cmd, Key::Num2, Do::View(View::Week)),
                (cmd, Key::Num3, Do::View(View::Day)),
            ] {
                if pressed(mods, key) {
                    actions.push(action);
                }
            }
        });
        // Plain keys, when nothing is being typed into.
        if !ctx.egui_wants_keyboard_input() {
            let n = self.config.places.len();
            let sel = self.config.selected;
            ctx.input_mut(|i| {
                let mut pressed = |key| i.consume_key(Modifiers::NONE, key);
                if pressed(Key::ArrowLeft) {
                    actions.push(Do::Step(-1));
                }
                if pressed(Key::ArrowRight) {
                    actions.push(Do::Step(1));
                }
                if pressed(Key::ArrowUp) && sel > 0 {
                    actions.push(Do::Select(sel - 1));
                }
                if pressed(Key::ArrowDown) && sel + 1 < n {
                    actions.push(Do::Select(sel + 1));
                }
                for (key, view) in [(Key::O, View::Overview), (Key::W, View::Week), (Key::D, View::Day)] {
                    if pressed(key) {
                        actions.push(Do::View(view));
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
                // Right to left: Overview, Week, Day read left to right.
                for (v, label, key) in
                    [(View::Day, "Day", "D"), (View::Week, "Week", "W"), (View::Overview, "Overview", "O")]
                {
                    if ui.selectable_label(view == v, label).on_hover_text(format!("{label} ({key})")).clicked() {
                        right.push(Do::View(v));
                    }
                }
                if view == View::Day {
                    ui.separator();
                    if ui.button("›").on_hover_text("Next day (→)").clicked() {
                        right.push(Do::Step(1));
                    }
                    if ui.button("‹").on_hover_text("Previous day (←)").clicked() {
                        right.push(Do::Step(-1));
                    }
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
        let has_places = !self.config.places.is_empty();
        ui.menu_button("File", |ui| {
            item(ui, actions, true, "Add Place…", "Ctrl+F", Do::Search);
            item(ui, actions, has_places, "Refresh", "Ctrl+R", Do::Refresh);
            ui.separator();
            item(ui, actions, true, "Quit", "Ctrl+Q", Do::Quit);
        });
        ui.menu_button("View", |ui| {
            item(ui, actions, true, "Overview", "Ctrl+1", Do::View(View::Overview));
            item(ui, actions, true, "Week", "Ctrl+2", Do::View(View::Week));
            item(ui, actions, true, "Day", "Ctrl+3", Do::View(View::Day));
            ui.separator();
            item(ui, actions, has_places, "Previous Day", "←", Do::Step(-1));
            item(ui, actions, has_places, "Next Day", "→", Do::Step(1));
            ui.separator();
            for n in [7, 10, 14] {
                if ui.radio(self.config.days == n, format!("{n} days ahead")).clicked() {
                    actions.push(Do::Days(n));
                }
            }
            ui.separator();
            let mut system = self.config.system_title_bar;
            if ui.checkbox(&mut system, "System title bar").changed() {
                actions.push(Do::SystemTitleBar(system));
            }
        });
        ui.menu_button("Units", |ui| {
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
            let units = self.config.units;
            ui.label(RichText::new("Temperature").weak().small());
            for (t, label) in Temperature::ALL {
                if ui.radio(units.temperature == t, label).clicked() {
                    actions.push(Do::Units(crate::units::Units { temperature: t, ..units }));
                }
            }
            ui.separator();
            ui.label(RichText::new("Wind").weak().small());
            for (s, label) in Speed::ALL {
                if ui.radio(units.speed == s, label).clicked() {
                    actions.push(Do::Units(crate::units::Units { speed: s, ..units }));
                }
            }
            ui.separator();
            ui.label(RichText::new("Rain, distance and pressure").weak().small());
            for (l, label) in Length::ALL {
                if ui.radio(units.length == l, label).clicked() {
                    actions.push(Do::Units(crate::units::Units { length: l, ..units }));
                }
            }
        });
        // Patina's skin menu (its ADR-0017): only controls; what they did goes to the status bar.
        ui.menu_button("Skins", |ui| {
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

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        let mut actions = Vec::new();
        ui.add_space(8.0);
        let search = ui.add(
            egui::TextEdit::singleline(&mut self.search).hint_text("Search for a city").desired_width(f32::INFINITY),
        );
        if std::mem::take(&mut self.focus_search) {
            search.request_focus();
        }
        let query = self.search.trim().to_string();
        // Search as you type, once there's enough to go on and the last search is back.
        if query.chars().count() >= 2
            && self.searching.is_none()
            && self.results.as_ref().is_none_or(|(q, _)| *q != query)
        {
            self.searching = Some(query.clone());
            self.fetcher.search(&query);
        }
        if query.is_empty() {
            self.results = None;
        }
        if search.has_focus() && ui.input(|i| i.key_pressed(Key::Escape)) {
            self.search.clear();
            self.results = None;
        }
        ui.add_space(6.0);

        if !query.is_empty() {
            match &self.results {
                Some((_, Ok(places))) if places.is_empty() && self.searching.is_none() => {
                    ui.label(RichText::new("No places found").weak());
                }
                Some((_, Ok(places))) => {
                    let enter = search.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                    if enter && let Some(first) = places.first() {
                        actions.push(Do::Add(first.clone()));
                    }
                    for p in places {
                        let row = ui.vertical(|ui| {
                            ui.set_width(ui.available_width());
                            ui.label(RichText::new(&p.name).strong());
                            ui.label(RichText::new(&p.region).small().weak());
                        });
                        let resp =
                            ui.interact(row.response.rect, ui.id().with(("result", p.key())), egui::Sense::click());
                        if resp.hovered() {
                            ui.painter().rect_filled(
                                row.response.rect.expand(2.0),
                                4.0,
                                ui.visuals().widgets.hovered.weak_bg_fill.gamma_multiply(0.5),
                            );
                        }
                        if resp.on_hover_text("Add this place").clicked() {
                            actions.push(Do::Add(p.clone()));
                        }
                    }
                }
                Some((_, Err(e))) => {
                    ui.colored_label(ui.visuals().error_fg_color, e);
                }
                None => {}
            }
            if self.searching.is_some() {
                ui.spinner();
            }
            ui.separator();
        }

        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            if self.config.places.is_empty() && query.is_empty() {
                ui.add_space(12.0);
                ui.label(RichText::new("Search for a city to add it.").weak());
            }
            let n = self.config.places.len();
            for (i, place) in self.config.places.iter().enumerate() {
                let forecast = self.forecasts.get(&place.key());
                let selected = i == self.config.selected;
                let resp = place_row(
                    ui,
                    place,
                    forecast,
                    selected,
                    self.loading.contains(&place.key()),
                    &self.config,
                    &self.palette,
                );
                if resp.clicked() {
                    actions.push(Do::Select(i));
                }
                resp.context_menu(|ui| {
                    if ui.add_enabled(i > 0, egui::Button::new("Move up")).clicked() {
                        actions.push(Do::Move(i, -1));
                        ui.close();
                    }
                    if ui.add_enabled(i + 1 < n, egui::Button::new("Move down")).clicked() {
                        actions.push(Do::Move(i, 1));
                        ui.close();
                    }
                    ui.separator();
                    if ui.button(RichText::new("Remove").color(ui.visuals().error_fg_color)).clicked() {
                        actions.push(Do::Remove(i));
                        ui.close();
                    }
                });
                ui.add_space(4.0);
            }
        });
        for a in actions {
            self.act(ui.ctx(), a);
        }
    }

    /// Patina's status bar (its ADR-0016): the version, then the latest
    /// message; on the right, when the forecast is from and who made it.
    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("v{}", env!("CARGO_PKG_VERSION"))).weak().small());
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
                // Open-Meteo's data is CC BY 4.0, which asks for this.
                ui.hyperlink_to(RichText::new("Weather data by Open-Meteo.com").small(), "https://open-meteo.com/");
                if !self.loading.is_empty() {
                    ui.spinner();
                    ui.label(RichText::new("Updating…").weak().small());
                } else if let Some(f) = self.forecast() {
                    let local = f.fetched.with_timezone(&chrono::Local);
                    ui.label(RichText::new(format!("Updated {}", local.format("%H:%M"))).weak().small()).on_hover_text(
                        format!("{} ({})", f.timezone, self.place().map(|p| p.region.as_str()).unwrap_or("")),
                    );
                }
            });
        });
    }

    fn content(&mut self, ui: &mut egui::Ui) {
        let Some(place) = self.place().cloned() else {
            ui.centered_and_justified(|ui| {
                ui.label(RichText::new("Search for a city in the sidebar to see its weather.").size(16.0).weak());
            });
            return;
        };
        let Some(forecast) = self.forecasts.get(&place.key()) else {
            ui.centered_and_justified(|ui| {
                if self.loading.contains(&place.key()) {
                    ui.spinner();
                } else {
                    ui.label(RichText::new(format!("No forecast for {} yet.", place.name)).weak());
                }
            });
            return;
        };
        let scene =
            Scene { forecast, place: &place, units: self.config.units, palette: &self.palette, days: self.config.days };
        let view = self.config.view;
        let day = self.shown_day();
        let action = egui::ScrollArea::vertical()
            .id_salt(("content", view as u8))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(4.0);
                let a = match (view, day) {
                    (View::Overview, _) => overview::show(ui, &scene),
                    (View::Week, _) => week::show(ui, &scene),
                    (View::Day, Some(d)) => day::show(ui, &scene, d),
                    (View::Day, None) => None,
                };
                ui.add_space(8.0);
                a
            })
            .inner;
        match action {
            Some(Action::ShowDay(d)) => self.act(ui.ctx(), Do::ShowDay(d)),
            None => {}
        }
    }

    fn take_screenshot(&mut self, ctx: &egui::Context) {
        let has_data = self.forecast().is_some() && self.loading.is_empty();
        let Some((path, requested, ready)) = &mut self.screenshot else { return };
        ctx.request_repaint();
        let now = ctx.input(|i| i.time);
        if ready.is_none() && (has_data || now > 10.0) {
            *ready = Some(now);
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
        } else if !*requested && ready.is_some_and(|t| now - t > 1.0) {
            *requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
    }
}

/// A place in the sidebar: its name, local time and sky on the left, the
/// temperature and the day's high and low on the right.
fn place_row(
    ui: &mut egui::Ui,
    place: &Place,
    forecast: Option<&Forecast>,
    selected: bool,
    loading: bool,
    config: &Config,
    palette: &Palette,
) -> egui::Response {
    let u = config.units;
    let v = ui.visuals().clone();
    let fill = if selected { v.selection.bg_fill } else { v.faint_bg_color };
    let text = if selected { v.selection.stroke.color } else { v.text_color() };
    let inner =
        egui::Frame::new().fill(fill).corner_radius(10).inner_margin(egui::Margin::symmetric(10, 8)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new(&place.name).strong().size(15.0).color(text));
                    let sub = match forecast {
                        Some(f) => f.now().format("%H:%M").to_string(),
                        None if loading => "Loading…".into(),
                        None => place.region.clone(),
                    };
                    ui.label(RichText::new(sub).small().color(text));
                    if let Some(f) = forecast {
                        ui.label(RichText::new(forecast::describe(f.current.code)).small().color(text));
                    }
                });
                if let Some(f) = forecast {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                        ui.vertical(|ui| {
                            ui.with_layout(egui::Layout::top_down(egui::Align::Max), |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(u.degrees(f.current.temperature)).size(26.0).color(text));
                                    icons::icon(ui, 26.0, f.current.code, f.current.is_day, palette);
                                });
                                let d = &f.days[0];
                                ui.label(
                                    RichText::new(format!("H:{}  L:{}", u.degrees(d.high), u.degrees(d.low)))
                                        .small()
                                        .color(text),
                                );
                            });
                        });
                    });
                }
            });
        });
    let resp = ui.interact(inner.response.rect, ui.id().with(("place", place.key())), egui::Sense::click());
    if resp.hovered() && !selected {
        ui.painter().rect_stroke(inner.response.rect, 10, v.widgets.hovered.bg_stroke, egui::StrokeKind::Inside);
    }
    resp.on_hover_text(&place.region)
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

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut changed = false;
        for reply in self.fetcher.poll() {
            match reply {
                Reply::Forecast { key, result } => {
                    self.loading.remove(&key);
                    match result {
                        Ok(f) => {
                            self.forecasts.insert(key, f);
                            changed = true;
                            if matches!(self.status, Some(Status { level: Level::Error, .. })) {
                                self.status = None;
                            }
                        }
                        Err(e) => self.status = Some(Status::error(e)),
                    }
                }
                Reply::Search { query, result } => {
                    self.searching = None;
                    self.results = Some((query, result));
                }
            }
        }
        if changed {
            fetch::save_cache(&self.forecasts);
        }
        // The light/dark switch can change under a skin that follows the desktop.
        if self.palette_dark != ctx.global_style().visuals.dark_mode {
            self.update_palette(ctx);
        }
        self.refresh_stale();
        self.shortcuts(ctx);
        // Keep the clock and "now" moving, and check for stale forecasts.
        ctx.request_repaint_after(StdDuration::from_secs(60));
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let custom = !self.config.system_title_bar;
        let style = self.skin().frame_style(ui.visuals());
        let radius = if custom { frame::corner_radius(&ctx, style.radius) } else { 0 };

        egui::Panel::top("header")
            .frame(frame::header_frame(ui.style(), &style, radius))
            .show(ui, |ui| self.header(ui, &style));

        egui::Panel::bottom("status")
            .frame(
                egui::Frame::side_top_panel(ui.style())
                    .inner_margin(egui::Margin::symmetric(10, 4))
                    .corner_radius(egui::CornerRadius { sw: radius, se: radius, ..Default::default() }),
            )
            .show(ui, |ui| self.status_bar(ui));

        egui::Panel::left("places")
            .frame(egui::Frame::side_top_panel(ui.style()).inner_margin(egui::Margin::symmetric(10, 4)))
            .default_size(260.0)
            .size_range(200.0..=420.0)
            .resizable(true)
            .show(ui, |ui| self.sidebar(ui));

        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(ui.style()).inner_margin(egui::Margin::symmetric(16, 10)))
            .show(ui, |ui| self.content(ui));

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
        patina::kinetic::glide(ctx, raw_input);
    }
}
