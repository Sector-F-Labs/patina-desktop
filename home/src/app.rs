//! The window: Patina's frame and header bar, a sidebar of entity domains,
//! and the entities of the chosen domain.

use std::collections::BTreeMap;

use patina::frame::{self, TitleBar};
use patina::skin::{self, Skin};

use crate::chart::{self, SensorDialog};
use crate::config::{self, Config};
use crate::dashboard::{self, Scene};
use crate::ha::{self, Account, Client, Command, Dashboard, Entity, Update};
use crate::history::{self, Store};
use crate::light::LightDialog;
use crate::palette::{self, Palette};
use crate::widgets::{Event, ServiceCall, entity_row};

/// The skin used until the user picks one: stock egui, following the desktop.
const HOME_SKIN: &str = "Default";

/// What the header asks for, handled after it's drawn.
enum Action {
    Quit,
    Refresh,
    Account,
    SetSkin(String),
    SystemTitleBar(bool),
}

/// The account dialog's fields, while it's open.
struct AccountDialog {
    draft: Account,
    error: Option<String>,
}

enum Connection {
    NotSetUp,
    Connecting,
    Connected,
    Failed(String),
    Demo,
}

pub struct App {
    config: Config,
    account: Account,
    client: Option<Client>,
    connection: Connection,
    entities: Vec<Entity>,
    dashboards: Vec<Dashboard>,
    /// The light whose settings are open, and the sensor whose graph is.
    light: Option<LightDialog>,
    sensor: Option<SensorDialog>,
    history: Store,
    palette: Palette,
    /// The skin `palette` was made for.
    palette_skin: String,
    /// The view tab showing in the chosen dashboard.
    view: usize,
    search: String,
    skins: Vec<Skin>,
    skin_errors: Vec<String>,
    /// `HOME_SKIN` env var: this run's skin, not saved.
    forced_skin: Option<String>,
    dialog: Option<AccountDialog>,
    /// Screenshot mode: where to save, and whether it's been asked for.
    screenshot: Option<(std::path::PathBuf, bool)>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        patina::appearance::watch(&cc.egui_ctx);
        let dirs: Vec<_> = skin::app_dir("home").into_iter().collect();
        let (skins, skin_errors) = skin::load_all(skin::BUILT_IN, &dirs);
        let account = config::load_account();
        let demo = std::env::var_os("HOME_DEMO").is_some();
        let mut app = Self {
            config: Config::load(),
            account,
            client: None,
            connection: Connection::NotSetUp,
            entities: Vec::new(),
            dashboards: Vec::new(),
            light: None,
            sensor: None,
            history: Store::default(),
            palette: Palette::new(),
            palette_skin: String::new(),
            view: 0,
            search: String::new(),
            skins,
            skin_errors,
            forced_skin: std::env::var("HOME_SKIN").ok(),
            dialog: None,
            screenshot: std::env::var_os("HOME_SCREENSHOT").map(|p| (p.into(), false)),
        };
        if demo {
            app.entities = ha::demo();
            app.dashboards = ha::demo_dashboards();
            app.select_default_dashboard();
            app.connection = Connection::Demo;
        } else {
            app.connect(&cc.egui_ctx);
        }
        if let Ok(id) = std::env::var("HOME_OPEN") {
            app.apply_events(vec![Event::Open(id)]); // for screenshots
        }
        app.apply_skin(&cc.egui_ctx);
        app
    }

    /// (Re)starts the worker for the current account; dropping the old client stops its thread.
    fn connect(&mut self, ctx: &egui::Context) {
        self.client = None;
        if self.account.is_set() {
            self.client = Some(Client::start(ctx, self.account.clone()));
            self.connection = Connection::Connecting;
        } else {
            self.connection = Connection::NotSetUp;
            self.entities.clear();
            self.dashboards.clear();
        }
    }

    /// A dashboard is what Home opens on, until the user picks the entity list.
    fn select_default_dashboard(&mut self) {
        if self.config.dashboard.is_none() && !self.config.entities && !self.dashboards.is_empty() {
            self.config.dashboard = Some(key(&self.dashboards[0]));
        }
    }

    fn dashboard(&self) -> Option<&Dashboard> {
        let chosen = self.config.dashboard.as_deref()?;
        self.dashboards.iter().find(|d| key(d) == chosen)
    }

    fn skin(&self) -> &Skin {
        let find = |name: &str| self.skins.iter().find(|s| s.name == name);
        let wanted = self.forced_skin.as_deref().or(self.config.skin.as_deref()).unwrap_or(HOME_SKIN);
        find(wanted).or_else(|| find(HOME_SKIN)).unwrap_or(&self.skins[0])
    }

    fn apply_skin(&self, ctx: &egui::Context) {
        // Replaces the whole egui style: re-apply any spacing of the app's own after it.
        self.skin().apply(ctx);
    }

    fn poll(&mut self) {
        while let Some(update) = self.client.as_ref().and_then(|c| c.rx.try_recv().ok()) {
            match update {
                Update::States(entities) => {
                    self.entities = entities;
                    self.connection = Connection::Connected;
                }
                Update::Dashboards(dashboards) => {
                    self.dashboards = dashboards;
                    self.select_default_dashboard();
                }
                Update::History { hours, series } => {
                    for (id, points) in series {
                        self.history.insert(&id, hours, points);
                    }
                }
                Update::Error(e) => {
                    self.history.clear_pending();
                    self.connection = Connection::Failed(e);
                }
            }
        }
    }

    /// Entities per domain, for the sidebar.
    fn domains(&self) -> BTreeMap<&str, usize> {
        let mut domains = BTreeMap::new();
        for e in &self.entities {
            *domains.entry(e.domain()).or_insert(0) += 1;
        }
        domains
    }

    fn call(&mut self, call: &ServiceCall) {
        if let Some(client) = &self.client {
            client.send(Command::Call {
                domain: call.domain.clone(),
                service: call.service.clone(),
                entity_id: call.entity_id.clone(),
                data: call.data.clone(),
            });
        }
    }

    /// Left side of the title bar: word menus, never a hamburger (Patina's ADR-0006).
    fn menus(&self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        ui.menu_button("File", |ui| {
            if ui.button("Account…").clicked() {
                actions.push(Action::Account);
                ui.close();
            }
            ui.separator();
            if ui.add(egui::Button::new("Quit").shortcut_text("Ctrl+Q")).clicked() {
                actions.push(Action::Quit);
                ui.close();
            }
        });
        ui.menu_button("View", |ui| {
            if ui.add(egui::Button::new("Refresh").shortcut_text("F5")).clicked() {
                actions.push(Action::Refresh);
                ui.close();
            }
            ui.separator();
            let mut system = self.config.system_title_bar;
            if ui.checkbox(&mut system, "System Title Bar").changed() {
                actions.push(Action::SystemTitleBar(system));
            }
        });
        ui.menu_button("Skins", |ui| {
            let current = self.skin().name.clone();
            for skin in &self.skins {
                if ui.radio(skin.name == current, &skin.name).clicked() {
                    actions.push(Action::SetSkin(skin.name.clone()));
                }
            }
            for error in &self.skin_errors {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
        });
    }

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        let domains: Vec<(String, usize)> = self.domains().into_iter().map(|(d, n)| (d.to_string(), n)).collect();
        let total = self.entities.len();
        let mut dashboard = self.config.dashboard.clone();
        let mut domain = self.config.domain.clone();
        let mut entities = self.config.entities;
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.set_width(ui.available_width());
            if !self.dashboards.is_empty() {
                ui.strong("Dashboards");
                for d in &self.dashboards {
                    let selected = dashboard.as_deref() == Some(key(d).as_str());
                    if ui.selectable_label(selected, &d.title).clicked() && !selected {
                        dashboard = Some(key(d));
                        entities = false;
                        self.view = 0;
                    }
                }
                ui.separator();
                ui.strong("Entities");
            }
            let listing = dashboard.is_none();
            if ui.selectable_label(listing && domain.is_none(), format!("All  ({total})")).clicked() {
                (dashboard, domain, entities) = (None, None, true);
            }
            for (name, count) in &domains {
                let selected = listing && domain.as_deref() == Some(name.as_str());
                if ui.selectable_label(selected, format!("{}  ({count})", domain_title(name))).clicked() {
                    (dashboard, domain, entities) = (None, Some(name.clone()), true);
                }
            }
        });
        if (&dashboard, &domain, entities) != (&self.config.dashboard, &self.config.domain, self.config.entities) {
            (self.config.dashboard, self.config.domain, self.config.entities) = (dashboard, domain, entities);
            self.config.save();
        }
    }

    /// The chosen dashboard: tabs for its views, then the view.
    fn dashboard_view(&mut self, ui: &mut egui::Ui) {
        let mut events = Vec::new();
        let mut wanted = Vec::new();
        if let Some(d) = self.dashboard() {
            match &d.config {
                Ok(config) => {
                    let titles = dashboard::view_titles(config);
                    let mut view = self.view.min(titles.len().saturating_sub(1));
                    if titles.len() > 1 {
                        ui.horizontal_wrapped(|ui| {
                            for (i, title) in titles.iter().enumerate() {
                                ui.selectable_value(&mut view, i, title);
                            }
                        });
                        ui.separator();
                    }
                    let mut scene = Scene {
                        states: self.entities.iter().map(|e| (e.id.as_str(), e)).collect(),
                        events: &mut events,
                        history: &self.history,
                        palette: &self.palette,
                        wanted: &mut wanted,
                    };
                    dashboard::show_view(ui, config, view, &mut scene);
                    self.view = view;
                }
                Err(e) => {
                    ui.colored_label(ui.visuals().error_fg_color, format!("{}: {e}", d.title));
                }
            }
        }
        self.apply_events(events);
        self.request_history(wanted);
    }

    /// Sends the service calls from clicks, shows their result until the next poll confirms it, and
    /// opens a light's settings.
    fn apply_events(&mut self, events: Vec<Event>) {
        for event in events {
            match event {
                Event::Open(id) => match self.entities.iter().find(|e| e.id == id) {
                    Some(e) if e.domain() == "light" => self.light = Some(LightDialog::new(id)),
                    Some(e) if chart::is_graphable(e) => self.sensor = Some(SensorDialog::new(id)),
                    _ => {}
                },
                Event::Call(call) => {
                    self.call(&call);
                    if let Some(e) = self.entities.iter_mut().find(|e| e.id == call.entity_id) {
                        if let Some(state) = call.state {
                            e.state = state;
                        }
                        e.attributes.extend(call.attributes);
                    }
                }
            }
        }
    }

    /// Fetches the history the cards and windows drew a graph for, unless it's fresh or on its way.
    fn request_history(&mut self, wanted: Vec<(String, u32)>) {
        let mut by_hours: std::collections::BTreeMap<u32, Vec<String>> = Default::default();
        for (id, hours) in wanted {
            let ids = by_hours.entry(hours).or_default();
            if self.history.wants(&id, hours) && !ids.contains(&id) {
                ids.push(id);
            }
        }
        for (hours, ids) in by_hours.into_iter().filter(|(_, ids)| !ids.is_empty()) {
            if matches!(self.connection, Connection::Demo) {
                let now = chrono::Utc::now().timestamp() as f64;
                for id in ids {
                    let current = self.entities.iter().find(|e| e.id == id).and_then(|e| e.state.parse().ok());
                    self.history.insert(&id, hours, history::demo(&id, hours, now, current.unwrap_or(0.0)));
                }
            } else if let Some(client) = &self.client {
                for id in &ids {
                    self.history.mark_pending(id, hours);
                }
                client.request_history(ids, hours);
            }
        }
    }

    fn sensor_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.sensor else { return };
        let mut wanted = Vec::new();
        let keep = match self.entities.iter().find(|e| e.id == dialog.id) {
            Some(e) => dialog.show(ctx, e, &self.history, &self.palette, &mut wanted),
            None => false,
        };
        if !keep {
            self.sensor = None;
        }
        self.request_history(wanted);
    }

    /// The graphs' colours: the defaults, with the skin's `[app.home]` over them.
    fn update_palette(&mut self) {
        let name = self.skin().name.clone();
        if name == self.palette_skin {
            return;
        }
        self.palette = match self.skin().app_section::<palette::Section>("home") {
            Ok((section, _warnings)) => Palette::new().with(section),
            Err(_) => Palette::new(),
        };
        self.palette_skin = name;
    }

    fn light_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.light else { return };
        let mut events = Vec::new();
        let keep = match self.entities.iter().find(|e| e.id == dialog.id) {
            Some(e) => dialog.show(ctx, e, &mut events),
            None => false,
        };
        if !keep {
            self.light = None;
        }
        self.apply_events(events);
    }

    fn entities_view(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("Search").desired_width(f32::INFINITY));
        });
        ui.add_space(6.0);

        if matches!(self.connection, Connection::NotSetUp) {
            ui.vertical_centered(|ui| {
                ui.add_space(60.0);
                ui.heading("Connect to Home Assistant");
                ui.label("Use File → Account… to enter its address and a long-lived access token.");
            });
            return;
        }

        let query = self.search.to_lowercase();
        let domain = self.config.domain.clone();
        let mut events = Vec::new();
        egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
            let mut current = "";
            for e in self.entities.iter().filter(|e| {
                domain.as_deref().is_none_or(|d| e.domain() == d)
                    && (query.is_empty() || e.name().to_lowercase().contains(&query) || e.id.contains(&query))
            }) {
                if domain.is_none() && e.domain() != current {
                    current = e.domain();
                    ui.add_space(8.0);
                    ui.strong(domain_title(current));
                    ui.separator();
                }
                entity_row(ui, e, None, &mut events);
            }
        });
        self.apply_events(events);
    }

    fn status_bar(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| match &self.connection {
            Connection::NotSetUp => ui.label("Not connected"),
            Connection::Connecting => ui.label("Connecting…"),
            Connection::Connected => ui.label(format!("Connected to {}", self.account.url.trim())),
            Connection::Failed(e) => ui.colored_label(ui.visuals().error_fg_color, e),
            Connection::Demo => ui.label("Demo entities"),
        });
    }

    fn account_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.dialog else { return };
        let (mut save, mut cancel) = (false, false);
        egui::Window::new("Account")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                egui::Grid::new("account").num_columns(2).spacing([8.0, 8.0]).show(ui, |ui| {
                    ui.label("Address");
                    ui.add(
                        egui::TextEdit::singleline(&mut dialog.draft.url)
                            .hint_text("http://homeassistant.local:8123")
                            .desired_width(300.0),
                    );
                    ui.end_row();
                    ui.label("Access token");
                    ui.add(egui::TextEdit::singleline(&mut dialog.draft.token).password(true).desired_width(300.0));
                    ui.end_row();
                });
                ui.small("A long-lived token from your Home Assistant profile. It's kept in a file only you can read.");
                if let Some(e) = &dialog.error {
                    ui.colored_label(ui.visuals().error_fg_color, e);
                }
                ui.horizontal(|ui| {
                    save = ui.button("Save").clicked();
                    cancel = ui.button("Cancel").clicked();
                });
            });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            cancel = true;
        }
        if save {
            match config::save_account(&dialog.draft) {
                Ok(()) => {
                    self.account = dialog.draft.clone();
                    self.dialog = None;
                    self.connect(ctx);
                }
                Err(e) => dialog.error = Some(e),
            }
        } else if cancel {
            self.dialog = None;
        }
    }

    fn take_screenshot(&mut self, ctx: &egui::Context) {
        let Some((path, requested)) = &mut self.screenshot else { return };
        ctx.request_repaint();
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
        } else if !*requested && ctx.input(|i| i.time) > 1.0 {
            *requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
    }
}

/// A dashboard's identity in the config: its URL path, empty for the default one.
fn key(d: &Dashboard) -> String {
    d.url_path.clone().unwrap_or_default()
}

/// "binary_sensor" → "Binary Sensor".
fn domain_title(domain: &str) -> String {
    domain
        .split('_')
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().chain(c).collect::<String>()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        if !self.config.system_title_bar {
            patina::frame::shadow::request(frame);
        }
        let ctx = ui.ctx().clone();
        self.poll();
        self.update_palette();
        // Every frame: Default's colours follow the desktop's light/dark setting.
        let style = self.skin().frame_style(ui.visuals());
        let custom = !self.config.system_title_bar;
        let radius = if custom { frame::corner_radius(&ctx, style.radius) } else { 0 };

        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Q)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        let mut actions = Vec::new();
        if ctx.input(|i| i.key_pressed(egui::Key::F5)) {
            actions.push(Action::Refresh);
        }

        egui::Panel::top("header").frame(frame::header_frame(ui.style(), &style, radius)).show(ui, |ui| {
            TitleBar::new("Home", &style).custom(custom).menu_bar(true).span(frame::header_span(ui)).show(
                ui,
                |ui| self.menus(ui, &mut actions),
                |_ui| {}, // a control app: nothing it makes goes on the right
            );
        });

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
            .default_size(200.0)
            .resizable(true)
            .show(ui, |ui| self.sidebar(ui));

        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(ui.style()))
            .show(ui, |ui| if self.dashboard().is_some() { self.dashboard_view(ui) } else { self.entities_view(ui) });

        for action in actions {
            match action {
                Action::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                Action::Refresh => {
                    if let Some(client) = &self.client {
                        client.send(Command::Refresh);
                        client.reload_dashboards();
                    }
                }
                Action::Account => {
                    self.dialog = Some(AccountDialog { draft: self.account.clone(), error: None });
                }
                Action::SetSkin(name) => {
                    self.config.skin = Some(name);
                    self.forced_skin = None;
                    self.config.save();
                    self.apply_skin(&ctx);
                }
                Action::SystemTitleBar(on) => {
                    self.config.system_title_bar = on;
                    self.config.save();
                    ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(on));
                }
            }
        }

        self.light_dialog(&ctx);
        self.sensor_dialog(&ctx);
        self.account_dialog(&ctx);
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
