//! The window: Patina's frame with menus in its header bar, the sidebar of
//! shows with search, the selected show's episodes, the player and the
//! status bar.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Duration;

use egui::{Key, KeyboardShortcut, Modifiers, RichText};
use patina::frame::{self, TitleBar};
use patina::skin::{self, Skin};

use crate::config::{Config, Listening};
use crate::download::Downloads;
use crate::feed::{self, Episode, Found, Show};
use crate::fetch::{self, Fetcher, Reply};
use crate::player::{Engine, PlayState};

/// Fetch a feed again once it's this old, in seconds.
const STALE_AFTER: i64 = 60 * 60;

/// How often the position is saved while playing, in seconds.
const SAVE_EVERY: f64 = 5.0;

/// The skin used until one is picked: stock egui, following the desktop.
const HOME_SKIN: &str = skin::DEFAULT_SKIN;

/// The app's name for its config folder, and so its skins folder.
const APP: &str = "podcasts";

/// Back and forward, in seconds, as most podcast players skip.
const BACK: i64 = 15;
const FORWARD: i64 = 30;

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

enum Art {
    Loading,
    Ready(PathBuf),
    Failed,
}

/// Something asked for from a menu, a button or a key, done once it's drawn.
enum Do {
    Search,
    Refresh,
    RefreshShow(String),
    Quit,
    Select(String),
    /// What was typed or pasted into the search: a feed or an Apple Podcasts link.
    AddLink(String),
    Subscribe(String),
    Unsubscribe(String),
    MarkAllPlayed(String),
    /// Show the episode's notes, or hide them.
    Open(String),
    Play(String, String),
    TogglePause,
    Skip(i64),
    Seek(f64),
    Volume(f32),
    Played(String, bool),
    Download(String, String),
    CancelDownload(String),
    DeleteDownload(String, String),
    ShowPlayed(bool),
    SetSkin(String),
    SetLightDark(String),
    ReloadSkins,
    CopySkins,
    OpenSkinsFolder,
    SystemTitleBar(bool),
}

pub struct App {
    config: Config,
    listening: Listening,
    skins: Vec<Skin>,
    skin_errors: Vec<String>,
    forced_skin: Option<String>,
    fetcher: Fetcher,
    downloads: Downloads,
    engine: Engine,
    /// By feed URL.
    shows: HashMap<String, Show>,
    loading: HashSet<String>,
    feed_errors: HashMap<String, String>,
    /// By artwork URL.
    art: HashMap<String, Art>,
    search: String,
    /// The query the results are for, and them.
    results: Option<(String, Result<Vec<Found>, String>)>,
    searching: Option<String>,
    looking_up: bool,
    focus_search: bool,
    /// The episode whose notes are showing.
    open: Option<String>,
    /// The episode in the player, as (feed URL, episode id), playing or not.
    now: Option<(String, String)>,
    /// A position being dragged to on the seek bar, in seconds.
    scrub: Option<f64>,
    saved_at: f64,
    status: Option<Status>,
    /// Screenshot mode: where to save, whether asked, and when data arrived.
    screenshot: Option<(PathBuf, bool, Option<f64>)>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let ctx = &cc.egui_ctx;
        patina::appearance::watch(ctx);
        egui_extras::install_image_loaders(ctx);
        let config = Config::load();
        let engine = Engine::new(config.volume);
        let mut app = Self::with(ctx, config, Listening::load(), fetch::load_shows(), engine);
        app.refresh_stale();
        app
    }

    /// The app with what it would load, and without fetching anything.
    fn with(
        ctx: &egui::Context,
        config: Config,
        listening: Listening,
        shows: HashMap<String, Show>,
        engine: Engine,
    ) -> Self {
        let (skins, skin_errors) = load_skins();
        let screenshot = std::env::var_os("PODCASTS_SCREENSHOT").map(|p| (PathBuf::from(p), false, None));
        let mut app = Self {
            now: config.last.clone(),
            config,
            listening,
            skins,
            skin_errors,
            forced_skin: std::env::var("PODCASTS_SKIN").ok(),
            fetcher: Fetcher::new(ctx),
            downloads: Downloads::new(ctx),
            engine,
            shows,
            loading: HashSet::new(),
            feed_errors: HashMap::new(),
            art: HashMap::new(),
            search: String::new(),
            results: None,
            searching: None,
            looking_up: false,
            focus_search: false,
            open: None,
            scrub: None,
            saved_at: 0.0,
            status: None,
            screenshot,
        };
        app.apply_skin(ctx);
        if app.config.subscriptions.is_empty() {
            app.focus_search = true;
        }
        if app.config.selected.is_none() {
            app.config.selected = app.config.subscriptions.first().cloned();
        }
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
    }

    fn show(&self) -> Option<&Show> {
        self.config.selected.as_ref().and_then(|url| self.shows.get(url))
    }

    fn episode(&self, show: &str, id: &str) -> Option<&Episode> {
        self.shows.get(show)?.episodes.iter().find(|e| e.id == id)
    }

    fn now_playing(&self) -> Option<(&Show, &Episode)> {
        let (url, id) = self.now.as_ref()?;
        let show = self.shows.get(url)?;
        Some((show, show.episodes.iter().find(|e| e.id == *id)?))
    }

    fn fetch(&mut self, url: &str) {
        if self.loading.insert(url.to_string()) {
            self.fetcher.feed(url);
        }
    }

    /// Fetches every feed that's missing or old.
    fn refresh_stale(&mut self) {
        let now = chrono::Utc::now();
        let stale: Vec<String> = self
            .config
            .subscriptions
            .iter()
            .filter(|u| !self.feed_errors.contains_key(*u))
            .filter(|u| self.shows.get(*u).is_none_or(|s| (now - s.fetched).num_seconds() > STALE_AFTER))
            .cloned()
            .collect();
        for url in stale {
            self.fetch(&url);
        }
    }

    /// Asks for the artwork of the shows and search results not asked for yet.
    fn request_art(&mut self) {
        let shows = self.config.subscriptions.iter().filter_map(|u| self.shows.get(u)?.artwork.clone());
        let found = self.results.iter().flat_map(|(_, r)| r.iter().flatten().filter_map(|f| f.artwork.clone()));
        let wanted: Vec<String> = shows.chain(found).filter(|u| !self.art.contains_key(u)).collect();
        for url in wanted {
            match fetch::art_file(&url).filter(|p| p.exists()) {
                Some(path) => self.art.insert(url, Art::Ready(path)),
                None => {
                    self.fetcher.art(&url);
                    self.art.insert(url, Art::Loading)
                }
            };
        }
    }

    fn art_path(&self, url: Option<&str>) -> Option<&PathBuf> {
        match self.art.get(url?)? {
            Art::Ready(path) => Some(path),
            Art::Loading | Art::Failed => None,
        }
    }

    /// Saves where the player is in the episode in it.
    fn save_position(&mut self) {
        let Some(id) = self.engine.current().map(str::to_string) else { return };
        if self.engine.state() == PlayState::Loading {
            return;
        }
        let duration = self.engine.duration().map(|d| d.as_secs_f64());
        self.listening.set_position(&id, self.engine.position().as_secs_f64(), duration);
        self.listening.save();
    }

    fn title(&self) -> String {
        self.show().map(|s| s.title.clone()).unwrap_or_else(|| "Podcasts".into())
    }

    fn act(&mut self, ctx: &egui::Context, action: Do) {
        match action {
            Do::Search => self.focus_search = true,
            Do::Refresh => {
                self.feed_errors.clear();
                for url in self.config.subscriptions.clone() {
                    self.fetch(&url);
                }
            }
            Do::RefreshShow(url) => {
                self.feed_errors.remove(&url);
                self.fetch(&url);
            }
            Do::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Do::Select(url) => {
                self.config.selected = Some(url);
                self.config.save();
                self.open = None;
            }
            Do::AddLink(text) => {
                let text = text.trim().to_string();
                if let Some(id) = feed::apple_id(&text) {
                    self.looking_up = true;
                    self.fetcher.lookup(&id);
                } else {
                    self.act(ctx, Do::Subscribe(text));
                }
            }
            Do::Subscribe(url) => {
                if !self.config.subscriptions.contains(&url) {
                    self.config.subscriptions.push(url.clone());
                    self.fetch(&url);
                }
                self.config.selected = Some(url);
                self.config.save();
                self.search.clear();
                self.results = None;
                self.open = None;
            }
            Do::Unsubscribe(url) => {
                self.config.subscriptions.retain(|u| *u != url);
                let name = self.shows.remove(&url).map(|s| s.title).unwrap_or_else(|| url.clone());
                fetch::save_shows(&self.shows);
                self.feed_errors.remove(&url);
                if self.config.selected.as_ref() == Some(&url) {
                    self.config.selected = self.config.subscriptions.first().cloned();
                }
                self.config.save();
                self.status = Some(Status::info(format!("Unsubscribed from {name}")));
            }
            Do::MarkAllPlayed(url) => {
                let ids: Vec<String> =
                    self.shows.get(&url).map(|s| s.episodes.iter().map(|e| e.id.clone()).collect()).unwrap_or_default();
                for id in ids {
                    self.listening.set_played(&id, true);
                }
                self.listening.save();
            }
            Do::Open(id) => self.open = if self.open.as_ref() == Some(&id) { None } else { Some(id) },
            Do::Play(show, id) => {
                if self.engine.current() == Some(id.as_str()) {
                    self.act(ctx, Do::TogglePause);
                    return;
                }
                let Some(episode) = self.episode(&show, &id).cloned() else { return };
                self.save_position();
                match self.downloads.open(&episode) {
                    Ok(audio) => {
                        let start = Duration::from_secs_f64(self.listening.resume_at(&id));
                        let duration = episode.duration.map(Duration::from_secs);
                        self.engine.play(&id, audio, episode.extension(), start, duration, ctx);
                        self.now = Some((show, id));
                        self.config.last = self.now.clone();
                        self.config.save();
                    }
                    Err(e) => self.status = Some(Status::error(e)),
                }
            }
            Do::TogglePause => match self.engine.state() {
                PlayState::Playing | PlayState::Paused => {
                    self.engine.toggle_pause();
                    self.save_position();
                }
                PlayState::Loading => {}
                PlayState::Stopped => {
                    if let Some((show, id)) = self.now.clone() {
                        self.act(ctx, Do::Play(show, id));
                    }
                }
            },
            Do::Skip(by) => {
                self.engine.skip(by);
                self.save_position();
            }
            Do::Seek(to) => {
                if self.engine.state() == PlayState::Stopped {
                    // Not loaded yet: move where it will start.
                    if let Some((_, id)) = &self.now {
                        let duration = self.now_playing().and_then(|(_, e)| e.duration).map(|d| d as f64);
                        let id = id.clone();
                        self.listening.set_position(&id, to, duration);
                        self.listening.save();
                    }
                } else {
                    self.engine.seek(Duration::from_secs_f64(to.max(0.0)));
                    self.save_position();
                }
            }
            Do::Volume(v) => {
                self.config.volume = v;
                self.engine.set_volume(v);
            }
            Do::Played(id, played) => {
                if played && self.engine.current() == Some(id.as_str()) {
                    self.engine.stop();
                }
                self.listening.set_played(&id, played);
                self.listening.save();
            }
            Do::Download(show, id) => {
                if let Some(e) = self.episode(&show, &id).cloned()
                    && let Err(e) = self.downloads.start(&e)
                {
                    self.status = Some(Status::error(e));
                }
            }
            Do::CancelDownload(id) => {
                if self.engine.current() == Some(id.as_str()) {
                    self.save_position();
                    self.engine.stop();
                }
                self.downloads.cancel(&id);
            }
            Do::DeleteDownload(show, id) => {
                if self.engine.current() == Some(id.as_str()) {
                    self.save_position();
                    self.engine.stop();
                }
                if let Some(e) = self.episode(&show, &id).cloned() {
                    self.downloads.delete(&e);
                }
            }
            Do::ShowPlayed(on) => {
                self.config.show_played = on;
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
                (cmd, Key::F, Do::Search),
                (cmd, Key::L, Do::Search),
                (cmd, Key::R, Do::Refresh),
                (cmd, Key::Q, Do::Quit),
            ] {
                if pressed(mods, key) {
                    actions.push(action);
                }
            }
        });
        // Plain keys, when nothing is being typed into.
        if !ctx.egui_wants_keyboard_input() {
            ctx.input_mut(|i| {
                let mut pressed = |key| i.consume_key(Modifiers::NONE, key);
                if pressed(Key::Space) {
                    actions.push(Do::TogglePause);
                }
                if pressed(Key::ArrowLeft) {
                    actions.push(Do::Skip(-BACK));
                }
                if pressed(Key::ArrowRight) {
                    actions.push(Do::Skip(FORWARD));
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
        let mut actions = Vec::new();
        // A player makes nothing, so the right of the bar stays empty (Patina's ADR-0006).
        TitleBar::new(&title, style).custom(custom).menu_bar(true).show(ui, |ui| self.menus(ui, &mut actions), |_| {});
        for a in actions {
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
        let has_shows = !self.config.subscriptions.is_empty();
        let selected = self.config.selected.clone();
        ui.menu_button("File", |ui| {
            item(ui, actions, true, "Add Podcast…", "Ctrl+F", Do::Search);
            item(ui, actions, has_shows, "Refresh All", "Ctrl+R", Do::Refresh);
            if let Some(url) = &selected {
                item(ui, actions, true, "Refresh This Podcast", "", Do::RefreshShow(url.clone()));
                ui.separator();
                item(ui, actions, true, "Mark All Played", "", Do::MarkAllPlayed(url.clone()));
                item(ui, actions, true, "Unsubscribe", "", Do::Unsubscribe(url.clone()));
            }
            ui.separator();
            item(ui, actions, true, "Quit", "Ctrl+Q", Do::Quit);
        });
        let has_now = self.now_playing().is_some();
        let playing = self.engine.state() == PlayState::Playing;
        ui.menu_button("Playback", |ui| {
            item(ui, actions, has_now, if playing { "Pause" } else { "Play" }, "Space", Do::TogglePause);
            item(ui, actions, has_now, &format!("Back {BACK} Seconds"), "←", Do::Skip(-BACK));
            item(ui, actions, has_now, &format!("Forward {FORWARD} Seconds"), "→", Do::Skip(FORWARD));
            if let Some((_, id)) = &self.now {
                ui.separator();
                item(ui, actions, has_now, "Mark Played", "", Do::Played(id.clone(), true));
            }
        });
        ui.menu_button("View", |ui| {
            let mut played = self.config.show_played;
            if ui.checkbox(&mut played, "Show played episodes").changed() {
                actions.push(Do::ShowPlayed(played));
            }
            ui.separator();
            let mut system = self.config.system_title_bar;
            if ui.checkbox(&mut system, "System title bar").changed() {
                actions.push(Do::SystemTitleBar(system));
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
            egui::TextEdit::singleline(&mut self.search)
                .hint_text("Search, or paste a feed or Apple link")
                .desired_width(f32::INFINITY),
        );
        if std::mem::take(&mut self.focus_search) {
            search.request_focus();
        }
        let query = self.search.trim().to_string();
        let is_link = query.starts_with("http://") || query.starts_with("https://");
        let enter = search.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        // Search as you type, once there's enough to go on and the last search is back.
        if !is_link
            && query.chars().count() >= 2
            && self.searching.is_none()
            && self.results.as_ref().is_none_or(|(q, _)| *q != query)
        {
            self.searching = Some(query.clone());
            self.fetcher.search(&query);
        }
        if query.is_empty() || is_link {
            self.results = None;
        }
        if search.has_focus() && ui.input(|i| i.key_pressed(Key::Escape)) {
            self.search.clear();
            self.results = None;
        }
        ui.add_space(6.0);

        if is_link {
            if ui.button("Subscribe to this link").clicked() || enter {
                actions.push(Do::AddLink(query.clone()));
            }
            ui.separator();
        } else if !query.is_empty() {
            match &self.results {
                Some((_, Ok(found))) if found.is_empty() && self.searching.is_none() => {
                    ui.label(RichText::new("No podcasts found").weak());
                }
                Some((_, Ok(found))) => {
                    if enter && let Some(first) = found.first() {
                        actions.push(Do::Subscribe(first.feed.clone()));
                    }
                    egui::ScrollArea::vertical().id_salt("results").max_height(ui.available_height() * 0.5).show(
                        ui,
                        |ui| {
                            for f in found {
                                let subscribed = self.config.subscriptions.contains(&f.feed);
                                let art = self.art_path(f.artwork.as_deref());
                                let sub = if subscribed { format!("✔ {}", f.author) } else { f.author.clone() };
                                let resp = show_row(ui, art, &f.title, &sub, false);
                                let hover = if subscribed { "Subscribed" } else { "Subscribe" };
                                if resp.on_hover_text(hover).clicked() {
                                    actions.push(Do::Subscribe(f.feed.clone()));
                                }
                            }
                        },
                    );
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
        if self.looking_up {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new("Finding the feed…").weak());
            });
        }

        egui::ScrollArea::vertical().id_salt("shows").auto_shrink([false, false]).show(ui, |ui| {
            if self.config.subscriptions.is_empty() && query.is_empty() {
                ui.add_space(12.0);
                ui.label(RichText::new("Search for a podcast, or paste its feed or Apple Podcasts link.").weak());
            }
            for url in &self.config.subscriptions {
                let show = self.shows.get(url);
                let selected = self.config.selected.as_ref() == Some(url);
                let title = show.map(|s| s.title.as_str()).unwrap_or(url);
                let sub = if let Some(e) = self.feed_errors.get(url) {
                    e.clone()
                } else if self.loading.contains(url) && show.is_none() {
                    "Loading…".into()
                } else {
                    show.map(|s| {
                        let unplayed = s.episodes.iter().filter(|e| !self.listening.get(&e.id).played).count();
                        format!("{unplayed} unplayed")
                    })
                    .unwrap_or_default()
                };
                let art = self.art_path(show.and_then(|s| s.artwork.as_deref()));
                let resp = show_row(ui, art, title, &sub, selected);
                if resp.clicked() {
                    actions.push(Do::Select(url.clone()));
                }
                resp.context_menu(|ui| {
                    if ui.button("Refresh").clicked() {
                        actions.push(Do::RefreshShow(url.clone()));
                        ui.close();
                    }
                    if ui.button("Mark All Played").clicked() {
                        actions.push(Do::MarkAllPlayed(url.clone()));
                        ui.close();
                    }
                    if ui.button("Copy Feed Link").clicked() {
                        ui.ctx().copy_text(url.clone());
                        ui.close();
                    }
                    ui.separator();
                    if ui.button(RichText::new("Unsubscribe").color(ui.visuals().error_fg_color)).clicked() {
                        actions.push(Do::Unsubscribe(url.clone()));
                        ui.close();
                    }
                });
            }
        });
        for a in actions {
            self.act(ui.ctx(), a);
        }
    }

    fn content(&mut self, ui: &mut egui::Ui) {
        let Some(url) = self.config.selected.clone() else {
            ui.centered_and_justified(|ui| {
                ui.label(RichText::new("Search for a podcast in the sidebar to subscribe to it.").size(16.0).weak());
            });
            return;
        };
        let Some(show) = self.shows.get(&url) else {
            ui.centered_and_justified(|ui| {
                if let Some(e) = self.feed_errors.get(&url) {
                    ui.colored_label(ui.visuals().error_fg_color, e);
                } else {
                    ui.spinner();
                }
            });
            return;
        };
        let mut actions = Vec::new();
        egui::ScrollArea::vertical().id_salt(("episodes", &url)).auto_shrink([false, false]).show(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal_top(|ui| {
                artwork(ui, self.art_path(show.artwork.as_deref()), 120.0);
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new(&show.title).size(22.0).strong());
                    if !show.author.is_empty() {
                        ui.label(RichText::new(&show.author).weak());
                    }
                    ui.add_space(4.0);
                    ui.add(egui::Label::new(&show.description).wrap()).on_hover_text(&show.description);
                    if !show.link.is_empty() {
                        ui.hyperlink_to(RichText::new("Website").small(), &show.link);
                    }
                });
            });
            ui.add_space(10.0);
            let played = show.episodes.iter().filter(|e| self.listening.get(&e.id).played).count();
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{} episodes, {played} played", show.episodes.len())).weak().small());
                if self.loading.contains(&url) {
                    ui.spinner();
                }
            });
            ui.separator();
            for e in &show.episodes {
                let progress = self.listening.get(&e.id);
                if progress.played && !self.config.show_played && self.engine.current() != Some(e.id.as_str()) {
                    continue;
                }
                self.episode_row(ui, &url, e, &mut actions);
                ui.separator();
            }
        });
        for a in actions {
            self.act(ui.ctx(), a);
        }
    }

    fn episode_row(&self, ui: &mut egui::Ui, show: &str, e: &Episode, actions: &mut Vec<Do>) {
        let progress = self.listening.get(&e.id);
        let current = self.engine.current() == Some(e.id.as_str());
        let playing = current && self.engine.state() == PlayState::Playing;
        let download = self.downloads.get(&e.id);
        let downloaded = self.downloads.is_downloaded(e);
        let open = self.open.as_ref() == Some(&e.id);

        let row = ui.horizontal_top(|ui| {
            let label = if playing { "⏸" } else { "▶" };
            let button = egui::Button::new(RichText::new(label).size(16.0)).min_size(egui::vec2(32.0, 32.0));
            if ui.add(button).on_hover_text(if playing { "Pause" } else { "Play" }).clicked() {
                actions.push(Do::Play(show.to_string(), e.id.clone()));
            }
            ui.vertical(|ui| {
                ui.set_width(ui.available_width());
                let mut title = RichText::new(&e.title).strong();
                if progress.played && !current {
                    title = title.weak();
                } else if current {
                    title = title.color(ui.visuals().selection.bg_fill);
                }
                ui.add(egui::Label::new(title).wrap());
                let mut facts = Vec::new();
                if let Some(d) = e.published {
                    facts.push(d.with_timezone(&chrono::Local).format("%-d %b %Y").to_string());
                }
                let position = if current { self.engine.position().as_secs_f64() } else { progress.position };
                match e.duration {
                    Some(d) if position > 1.0 && !progress.played => {
                        facts.push(format!("{} left", minutes((d as f64 - position).max(0.0))))
                    }
                    Some(d) => facts.push(minutes(d as f64)),
                    None => {}
                }
                if progress.played {
                    facts.push("Played".into());
                }
                if let Some(d) = download {
                    facts.push(match d.fraction() {
                        Some(f) => format!("Downloading {:.0}%", f * 100.0),
                        None => "Downloading".into(),
                    });
                } else if downloaded {
                    facts.push("Downloaded".into());
                }
                ui.label(RichText::new(facts.join(" · ")).small().weak());
                if let Some(d) = e.duration.filter(|d| *d > 0)
                    && position > 1.0
                    && !progress.played
                {
                    let bar = egui::ProgressBar::new((position / d as f64) as f32).desired_height(3.0);
                    ui.add(bar.desired_width(ui.available_width().min(240.0)));
                }
                if open && !e.notes.is_empty() {
                    ui.add_space(4.0);
                    ui.add(egui::Label::new(&e.notes).wrap().selectable(true));
                } else if !e.notes.is_empty() {
                    let first = e.notes.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
                    ui.add(egui::Label::new(RichText::new(first).small()).truncate());
                }
            });
        });
        let resp = ui.interact(row.response.rect, ui.id().with(("episode", &e.id)), egui::Sense::click());
        if resp.clicked() {
            actions.push(Do::Open(e.id.clone()));
        }
        if resp.double_clicked() {
            actions.push(Do::Play(show.to_string(), e.id.clone()));
        }
        resp.context_menu(|ui| {
            let mut item = |ui: &mut egui::Ui, label: &str, action: Do| {
                if ui.button(label).clicked() {
                    actions.push(action);
                    ui.close();
                }
            };
            item(ui, if playing { "Pause" } else { "Play" }, Do::Play(show.to_string(), e.id.clone()));
            item(ui, if open { "Hide Notes" } else { "Show Notes" }, Do::Open(e.id.clone()));
            if progress.played {
                item(ui, "Mark Unplayed", Do::Played(e.id.clone(), false));
            } else {
                item(ui, "Mark Played", Do::Played(e.id.clone(), true));
            }
            ui.separator();
            if download.is_some() {
                item(ui, "Cancel Download", Do::CancelDownload(e.id.clone()));
            } else if downloaded {
                item(ui, "Delete Download", Do::DeleteDownload(show.to_string(), e.id.clone()));
            } else {
                item(ui, "Download", Do::Download(show.to_string(), e.id.clone()));
            }
            if ui.button("Copy Audio Link").clicked() {
                ui.ctx().copy_text(e.audio.clone());
                ui.close();
            }
        });
    }

    /// The episode in the player: its controls, where it's at, and the volume.
    fn player(&mut self, ui: &mut egui::Ui) {
        let Some((show, episode)) = self.now_playing() else {
            ui.label(RichText::new("Nothing playing").weak());
            return;
        };
        let state = self.engine.state();
        let active = state != PlayState::Stopped;
        let position =
            if active { self.engine.position().as_secs_f64() } else { self.listening.resume_at(&episode.id) };
        let duration = if active { self.engine.duration() } else { None }
            .map(|d| d.as_secs_f64())
            .or(episode.duration.map(|d| d as f64));
        let mut actions = Vec::new();
        let mut scrub = self.scrub;
        ui.horizontal(|ui| {
            artwork(ui, self.art_path(show.artwork.as_deref()), 44.0);
            ui.vertical(|ui| {
                ui.set_width(220.0);
                ui.add(egui::Label::new(RichText::new(&episode.title).strong()).truncate());
                ui.add(egui::Label::new(RichText::new(&show.title).small().weak()).truncate());
            });
            ui.add_space(8.0);
            if ui.button(format!("↺ {BACK}")).on_hover_text(format!("Back {BACK} seconds (←)")).clicked() {
                actions.push(Do::Skip(-BACK));
            }
            let playing = state == PlayState::Playing;
            let label = if playing { "⏸" } else { "▶" };
            let play = egui::Button::new(RichText::new(label).size(18.0)).min_size(egui::vec2(40.0, 32.0));
            if ui.add_enabled(state != PlayState::Loading, play).on_hover_text("Play or pause (Space)").clicked() {
                actions.push(Do::TogglePause);
            }
            if ui.button(format!("{FORWARD} ↻")).on_hover_text(format!("Forward {FORWARD} seconds (→)")).clicked() {
                actions.push(Do::Skip(FORWARD));
            }
            ui.add_space(8.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut volume = self.config.volume;
                ui.spacing_mut().slider_width = 80.0;
                if ui.add(egui::Slider::new(&mut volume, 0.0..=1.0).show_value(false)).on_hover_text("Volume").changed()
                {
                    actions.push(Do::Volume(volume));
                }
                ui.label("🔈");
                ui.add_space(8.0);
                let total = duration.unwrap_or(0.0);
                ui.label(RichText::new(clock(total)).monospace().small());
                let elapsed_width = 60.0;
                ui.spacing_mut().slider_width = (ui.available_width() - elapsed_width).max(60.0);
                let mut at = scrub.unwrap_or(position).min(total.max(position));
                let slider = egui::Slider::new(&mut at, 0.0..=total.max(1.0)).show_value(false);
                let resp = ui.add_enabled(duration.is_some(), slider);
                if resp.dragged() {
                    scrub = Some(at);
                } else if resp.drag_stopped() || resp.changed() {
                    scrub = None;
                    actions.push(Do::Seek(at));
                }
                let elapsed = if self.engine.buffering() { "…".to_string() } else { clock(at) };
                ui.label(RichText::new(elapsed).monospace().small()).on_hover_text(if self.engine.buffering() {
                    "Buffering"
                } else {
                    ""
                });
            });
        });
        self.scrub = scrub;
        for a in actions {
            self.act(ui.ctx(), a);
        }
    }

    /// Patina's status bar (its ADR-0016): the version, then the latest
    /// message; on the right, what's downloading or loading.
    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("v{}", env!("CARGO_PKG_VERSION"))).weak().small());
            let skin_errors = (!self.skin_errors.is_empty()).then(|| {
                let n = self.skin_errors.len();
                Status::error(format!("Couldn't load {n} skin{}", if n == 1 { "" } else { "s" }))
            });
            let engine_error = self.engine.error.as_ref().map(|e| Status::error(e.clone()));
            if let Some(status) = skin_errors.as_ref().or(engine_error.as_ref()).or(self.status.as_ref()) {
                ui.separator();
                let color = match status.level {
                    Level::Error => ui.visuals().error_fg_color,
                    Level::Info => ui.visuals().weak_text_color(),
                };
                let resp = ui.label(RichText::new(&status.text).color(color).small());
                if skin_errors.is_some() {
                    resp.on_hover_text(self.skin_errors.join("\n"));
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.engine.buffering() {
                    ui.label(RichText::new("Buffering…").weak().small());
                    ui.spinner();
                } else if !self.loading.is_empty() {
                    ui.label(RichText::new("Updating…").weak().small());
                    ui.spinner();
                } else if !self.downloads.is_empty() {
                    ui.label(RichText::new("Downloading…").weak().small());
                    ui.spinner();
                }
            });
        });
    }

    fn take_screenshot(&mut self, ctx: &egui::Context) {
        let has_data =
            self.show().is_some() && self.loading.is_empty() && !self.art.values().any(|a| matches!(a, Art::Loading));
        let Some((path, requested, ready)) = &mut self.screenshot else { return };
        ctx.request_repaint();
        let now = ctx.input(|i| i.time);
        if ready.is_none() && (has_data || now > 15.0) {
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
        } else if !*requested && ready.is_some_and(|t| now - t > 1.5) {
            *requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
        // Something in the player for the picture.
        if self.now.is_none()
            && let Some(show) = self.show()
            && let Some(first) = show.episodes.first()
        {
            self.now = Some((show.url.clone(), first.id.clone()));
        }
    }
}

/// A show's artwork, or a plain tile until it's here.
fn artwork(ui: &mut egui::Ui, path: Option<&PathBuf>, size: f32) {
    let radius = (size / 10.0).clamp(3.0, 10.0);
    match path {
        Some(path) => {
            ui.add(
                egui::Image::new(format!("file://{}", path.display()))
                    .fit_to_exact_size(egui::vec2(size, size))
                    .corner_radius(radius),
            );
        }
        None => {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
            let v = ui.visuals();
            ui.painter().rect_filled(rect, radius, v.faint_bg_color);
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "🎙",
                egui::FontId::proportional(size * 0.45),
                v.weak_text_color(),
            );
        }
    }
}

/// A show in the sidebar, subscribed to or found.
fn show_row(ui: &mut egui::Ui, art: Option<&PathBuf>, title: &str, sub: &str, selected: bool) -> egui::Response {
    let v = ui.visuals().clone();
    let fill = if selected { v.selection.bg_fill } else { egui::Color32::TRANSPARENT };
    let text = if selected { v.selection.stroke.color } else { v.text_color() };
    let inner =
        egui::Frame::new().fill(fill).corner_radius(8).inner_margin(egui::Margin::symmetric(6, 5)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                artwork(ui, art, 40.0);
                ui.vertical(|ui| {
                    ui.add(egui::Label::new(RichText::new(title).strong().color(text)).truncate());
                    ui.add(egui::Label::new(RichText::new(sub).small().color(text)).truncate());
                });
            });
        });
    let resp = ui.interact(inner.response.rect, ui.id().with(("show", title, sub)), egui::Sense::click());
    if resp.hovered() && !selected {
        ui.painter().rect_stroke(inner.response.rect, 8, v.widgets.hovered.bg_stroke, egui::StrokeKind::Inside);
    }
    resp
}

/// `1:02:03` or `23:41`.
pub fn clock(seconds: f64) -> String {
    let s = seconds.max(0.0) as u64;
    let (h, m, s) = (s / 3600, s / 60 % 60, s % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

/// `23 min` or `1 h 2 min`.
pub fn minutes(seconds: f64) -> String {
    let m = (seconds / 60.0).round().max(1.0) as u64;
    if m >= 60 { format!("{} h {} min", m / 60, m % 60) } else { format!("{m} min") }
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
                Reply::Feed { url, result } => {
                    self.loading.remove(&url);
                    match result {
                        Ok(show) => {
                            self.feed_errors.remove(&url);
                            self.shows.insert(url, show);
                            changed = true;
                        }
                        Err(e) => {
                            self.feed_errors.insert(url, e);
                        }
                    }
                }
                Reply::Search { query, result } => {
                    self.searching = None;
                    self.results = Some((query, result));
                }
                Reply::Lookup { result } => {
                    self.looking_up = false;
                    match result {
                        Ok(feed) => self.act(ctx, Do::Subscribe(feed)),
                        Err(e) => self.status = Some(Status::error(e)),
                    }
                }
                Reply::Art { url, result } => {
                    self.art.insert(url, result.map_or(Art::Failed, Art::Ready));
                }
            }
        }
        if changed {
            fetch::save_shows(&self.shows);
        }
        for (_, error) in self.downloads.poll() {
            if let Some(e) = error.filter(|e| e != "Cancelled") {
                self.status = Some(Status::error(e));
            }
        }
        self.engine.poll();
        if let Some(id) = self.engine.take_finished() {
            self.listening.set_played(&id, true);
            self.listening.save();
        }
        let now = ctx.input(|i| i.time);
        if self.engine.state() == PlayState::Playing && now - self.saved_at > SAVE_EVERY {
            self.saved_at = now;
            self.save_position();
        }
        self.refresh_stale();
        self.request_art();
        self.shortcuts(ctx);
        if self.engine.state() != PlayState::Stopped || !self.downloads.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(250));
        } else {
            ctx.request_repaint_after(Duration::from_secs(60));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.draw(ui);
    }

    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        frame::clear_color(visuals, !self.config.system_title_bar)
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        frame::release_grab(ctx, raw_input);
        patina::kinetic::glide(ctx, raw_input);
    }

    fn on_exit(&mut self) {
        self.save_position();
        self.config.save();
    }
}

impl App {
    fn draw(&mut self, ui: &mut egui::Ui) {
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

        egui::Panel::bottom("player")
            .frame(egui::Frame::side_top_panel(ui.style()).inner_margin(egui::Margin::symmetric(12, 8)))
            .show(ui, |ui| self.player(ui));

        egui::Panel::left("shows")
            .frame(egui::Frame::side_top_panel(ui.style()).inner_margin(egui::Margin::symmetric(10, 4)))
            .default_size(280.0)
            .size_range(200.0..=440.0)
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times() {
        assert_eq!(clock(83.0), "1:23");
        assert_eq!(clock(3723.0), "1:02:03");
        assert_eq!(minutes(1380.0), "23 min");
        assert_eq!(minutes(3720.0), "1 h 2 min");
        assert_eq!(minutes(10.0), "1 min");
    }

    /// Draws the window with a real feed in it, in every state the player
    /// has without audio, and clicks an episode open.
    #[test]
    fn draws_with_a_show() {
        let url = "https://example.com/feed".to_string();
        let show = feed::parse(&url, include_bytes!("../testdata/coffee-break-swedish.xml")).unwrap();
        let first = show.episodes[0].id.clone();
        let config = Config { subscriptions: vec![url.clone()], selected: Some(url.clone()), ..Config::default() };
        let mut listening = Listening::default();
        listening.set_position(&show.episodes[1].id, 300.0, Some(1800.0));
        listening.set_played(&show.episodes[2].id, true);
        let ctx = egui::Context::default();
        let mut app = App::with(&ctx, config, listening, HashMap::from([(url.clone(), show)]), Engine::silent());
        let frame = |app: &mut App, events: Vec<egui::Event>| {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1100.0, 760.0))),
                events,
                ..Default::default()
            };
            let _ = ctx.run_ui(input, |ui| app.draw(ui));
        };
        frame(&mut app, vec![]);
        assert!(app.now.is_none());
        app.act(&ctx, Do::Open(first.clone()));
        app.now = Some((url.clone(), first.clone()));
        frame(&mut app, vec![]);
        frame(&mut app, vec![]);
        assert_eq!(app.title(), "Coffee Break Swedish");
        // Set directly: acting would save the config, and tests never touch yours.
        app.config.show_played = false;
        frame(&mut app, vec![]);
    }
}
