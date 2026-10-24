//! The window: Patina's frame with menus and the message actions in its
//! header bar, the folder sidebar, the message list, the reading pane, and
//! the compose window and account dialog.

use std::collections::{HashMap, HashSet};

use egui::{Key, KeyboardShortcut, Modifiers, RichText};
use patina::frame::{self, TitleBar};
use patina::skin::{self, Skin};

use crate::compose::Draft;
use crate::config::{self, Account, Config, Security};
use crate::html::{self, Click, Event, Page};
use crate::message::{self, Body, Order, SortBy, Summary};
use crate::remote::{self, Fetcher, RemoteImage};
use crate::sync::{Command, Flag, Folder, Sync, Update};

/// Check Bridge for new mail this often while the app is open.
const REFRESH_EVERY: f64 = 120.0;

/// The skin used until the user picks one: stock egui, following the desktop.
const HOME_SKIN: &str = "Default";

/// The app's name for its config folder, and so its skins folder.
const APP: &str = "mail";

fn load_skins() -> (Vec<Skin>, Vec<String>) {
    skin::load_all(skin::BUILT_IN, &skin::app_dir(APP).into_iter().collect::<Vec<_>>())
}

const NEW: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::N);
const REPLY: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::R);
const REPLY_ALL: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::R);
const FORWARD: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::L);
const UNREAD: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::U);
const QUIT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Q);
const SEND: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Enter);

/// A message for the status bar.
struct Status {
    error: bool,
    text: String,
}

impl Status {
    fn error(text: impl Into<String>) -> Self {
        Status { error: true, text: text.into() }
    }

    fn info(text: impl Into<String>) -> Self {
        Status { error: false, text: text.into() }
    }
}

/// The open message drawn by Blitz: which message, from which remote-image
/// state, and the remote images `remote::prepare` found in it.
struct OpenPage {
    folder: String,
    uid: u32,
    version: u64,
    page: Page,
    images: Vec<RemoteImage>,
}

/// The open message's remote images (ADR-0006): which the user approved,
/// and what has come back. Choices last only while the message is open.
#[derive(Default)]
struct Remote {
    key: Option<(String, u32)>,
    approved: HashSet<String>,
    fetched: HashMap<String, Vec<u8>>,
    pending: HashSet<String>,
    failed: HashMap<String, String>,
    /// Goes up with every change, so the page knows to rebuild.
    version: u64,
}

/// What the menus, header, list and keys ask for, handled in one place.
#[derive(Debug, Clone, PartialEq)]
enum Do {
    Quit,
    Refresh,
    Account,
    Open(String),
    Select(u32),
    /// Up or down the list.
    Step(i32),
    New,
    Reply {
        all: bool,
    },
    Forward,
    MarkUnread,
    ToggleFlag,
    /// The selected message to Trash, or to Archive.
    MoveTo(&'static str),
    SaveAttachment(usize),
    ShowText(bool),
    Sort(Order),
    /// A link clicked in an HTML message: ask before opening it.
    Link(Click),
    OpenLink(String),
    /// Fetch these remote images for the open message (ADR-0006).
    LoadImages(Vec<String>),
    ReviewImages,
    CopyText(String),
    Cut,
    Copy,
    Paste,
    SetSkin(String),
    SetLightDark(String),
    ReloadSkins,
    CopySkins,
    OpenSkinsFolder,
    SystemTitleBar(bool),
}

struct Compose {
    draft: Draft,
    sending: bool,
    error: Option<String>,
    /// Ask the body for focus once it's drawn.
    focus_body: bool,
}

pub struct App {
    config: Config,
    account: Account,
    skins: Vec<Skin>,
    skin_errors: Vec<String>,
    /// From `MAIL_SKIN`, for screenshots: used without being saved.
    forced_skin: Option<String>,
    sync: Sync,
    folders: Vec<Folder>,
    messages: Vec<Summary>,
    /// Which folder `messages` came from.
    listed: Option<String>,
    selected: Option<u32>,
    bodies: HashMap<(String, u32), Body>,
    compose: Option<Compose>,
    page: Option<OpenPage>,
    remote: Remote,
    fetcher: Fetcher,
    /// The Review… list of remote images, with what's ticked.
    review: Option<Vec<bool>>,
    /// `MAIL_DEMO=review`: open the list once the page is drawn, for screenshots.
    demo_review: bool,
    /// A clicked link waiting for the user to open it, copy it or not.
    link: Option<Click>,
    /// The account dialog, with what's typed so far.
    account_dialog: Option<Account>,
    status: Option<Status>,
    last_refresh: f64,
    scroll_to_selected: bool,
    /// Screenshot mode: where to save, and whether it was asked for.
    screenshot: Option<(std::path::PathBuf, bool)>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        patina::appearance::watch(&cc.egui_ctx);
        let config = Config::load();
        // MAIL_DEMO=1 shows made-up mail; MAIL_DEMO=reply also opens a reply to it.
        let demo_mode = std::env::var("MAIL_DEMO").ok();
        let demo = demo_mode.is_some();
        let account = if demo { crate::demo::account() } else { config::load_account() };
        let sync = if demo { Sync::offline(&cc.egui_ctx) } else { Sync::start(&cc.egui_ctx, account.clone()) };
        let (skins, skin_errors) = load_skins();
        let mut app = Self {
            config,
            account,
            skins,
            skin_errors,
            forced_skin: std::env::var("MAIL_SKIN").ok(),
            sync,
            folders: Vec::new(),
            messages: Vec::new(),
            listed: None,
            selected: None,
            bodies: HashMap::new(),
            compose: None,
            page: None,
            remote: Remote::default(),
            fetcher: Fetcher::new(&cc.egui_ctx),
            review: None,
            demo_review: false,
            link: None,
            account_dialog: None,
            status: None,
            last_refresh: 0.0,
            scroll_to_selected: false,
            screenshot: std::env::var_os("MAIL_SCREENSHOT").map(|p| (p.into(), false)),
        };
        app.apply_skin(&cc.egui_ctx);
        if demo {
            app.load_demo();
            app.demo_review = demo_mode.as_deref() == Some("review");
            if demo_mode.as_deref() == Some("reply") {
                app.act(&cc.egui_ctx, Do::Reply { all: false });
            }
        } else if app.account.is_set() {
            app.sync.send(Command::Refresh);
            app.sync.send(Command::Open(app.config.folder.clone()));
        } else if app.screenshot.is_none() {
            app.account_dialog = Some(Account::default());
        }
        app
    }

    fn load_demo(&mut self) {
        let (folders, messages, bodies) = crate::demo::mailbox();
        self.folders = folders;
        self.config.folder = "INBOX".into();
        self.listed = Some("INBOX".into());
        self.selected = messages.first().map(|m| m.uid);
        self.bodies = bodies.into_iter().map(|(uid, b)| (("INBOX".to_string(), uid), b)).collect();
        self.messages = messages;
        self.config.order.sort(&mut self.messages);
    }

    fn skin(&self) -> &Skin {
        let name = self.forced_skin.as_deref().or(self.config.skin.as_deref()).unwrap_or(HOME_SKIN);
        let find = |name: &str| self.skins.iter().find(|s| s.name == name);
        find(name).or_else(|| find(HOME_SKIN)).unwrap_or(&self.skins[0])
    }

    fn apply_skin(&self, ctx: &egui::Context) {
        let skin = self.skin();
        skin.apply(ctx);
        if skin.follows_desktop() {
            ctx.set_theme(self.config.theme());
        }
    }

    fn folder(&self) -> &str {
        &self.config.folder
    }

    /// The open message, if it's been fetched.
    fn body(&self) -> Option<&Body> {
        self.bodies.get(&(self.config.folder.clone(), self.selected?))
    }

    /// The selected message's index in the list.
    fn index(&self) -> Option<usize> {
        let uid = self.selected?;
        self.messages.iter().position(|m| m.uid == uid)
    }

    /// The folder Bridge keeps a standard mailbox in, by its usual name.
    fn find_folder(&self, name: &str) -> Option<String> {
        self.folders.iter().find(|f| f.name.eq_ignore_ascii_case(name)).map(|f| f.name.clone())
    }

    fn poll(&mut self) {
        for (url, result) in self.fetcher.poll() {
            let r = &mut self.remote;
            // Results for a message that's no longer open are dropped.
            if !r.pending.remove(&url) {
                continue;
            }
            match result {
                Ok(bytes) => {
                    r.fetched.insert(url, bytes);
                }
                Err(e) => {
                    r.failed.insert(url, e);
                }
            }
            r.version += 1;
        }
        for update in self.sync.poll() {
            match update {
                Update::Busy(_) => {}
                Update::Folders(folders) => {
                    self.folders = folders;
                    if self.status.as_ref().is_some_and(|s| s.error) {
                        self.status = None;
                    }
                }
                Update::Messages { folder, messages } => {
                    if folder != self.config.folder {
                        continue;
                    }
                    self.messages = messages;
                    self.config.order.sort(&mut self.messages);
                    self.listed = Some(folder);
                    if self.selected.is_some() && self.index().is_none() {
                        self.selected = None;
                    }
                }
                Update::Body { folder, uid, body } => {
                    self.bodies.insert((folder, uid), *body);
                }
                Update::Sent => {
                    self.compose = None;
                    self.status = Some(Status::info("Sent"));
                }
                Update::Error(e) => {
                    let sending = self.compose.as_mut().filter(|c| c.sending && e.starts_with("Not sent"));
                    match sending {
                        Some(c) => {
                            c.sending = false;
                            c.error = Some(e);
                        }
                        None => self.status = Some(Status::error(e)),
                    }
                }
            }
        }
    }

    fn act(&mut self, ctx: &egui::Context, action: Do) {
        match action {
            Do::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Do::Refresh => {
                self.sync.send(Command::Refresh);
                self.last_refresh = ctx.input(|i| i.time);
            }
            Do::Account => self.account_dialog = Some(self.account.clone()),
            Do::Open(folder) => {
                if folder != self.config.folder {
                    self.config.folder = folder.clone();
                    self.config.save();
                    self.messages.clear();
                    self.listed = None;
                    self.selected = None;
                }
                self.sync.send(Command::Open(folder));
            }
            Do::Select(uid) => {
                if self.selected != Some(uid) {
                    self.remote = Remote::default();
                    self.review = None;
                }
                self.selected = Some(uid);
                let folder = self.folder().to_string();
                if !self.bodies.contains_key(&(folder.clone(), uid)) {
                    self.sync.send(Command::Read { folder: folder.clone(), uid });
                }
                if let Some(m) = self.messages.iter_mut().find(|m| m.uid == uid)
                    && !m.seen
                {
                    m.seen = true;
                    if let Some(f) = self.folders.iter_mut().find(|f| f.name == folder) {
                        f.unseen = f.unseen.saturating_sub(1);
                    }
                }
            }
            Do::Step(by) => {
                let next = match self.index() {
                    Some(i) => (i as i64 + by as i64).clamp(0, self.messages.len() as i64 - 1) as usize,
                    None => 0,
                };
                if let Some(uid) = self.messages.get(next).map(|m| m.uid) {
                    self.scroll_to_selected = true;
                    self.act(ctx, Do::Select(uid));
                }
            }
            Do::New => self.start_compose(Draft::default()),
            Do::Reply { all } => {
                if let Some(body) = self.body() {
                    let draft = Draft::reply(body, &self.account.username, all);
                    self.start_compose(draft);
                }
            }
            Do::Forward => {
                if let Some(body) = self.body() {
                    let draft = Draft::forward(body);
                    self.start_compose(draft);
                }
            }
            Do::MarkUnread => self.set_flag(Flag::Seen, false),
            Do::ToggleFlag => {
                let flagged = self.index().map(|i| self.messages[i].flagged).unwrap_or(false);
                self.set_flag(Flag::Flagged, !flagged);
            }
            Do::MoveTo(name) => {
                let (Some(uid), Some(to)) = (self.selected, self.find_folder(name)) else { return };
                if to == self.config.folder {
                    return;
                }
                // Go on to the next message, as mail apps do.
                let i = self.index().unwrap_or(0);
                self.messages.retain(|m| m.uid != uid);
                self.selected = self.messages.get(i.min(self.messages.len().saturating_sub(1))).map(|m| m.uid);
                if let Some(next) = self.selected {
                    self.act(ctx, Do::Select(next));
                }
                self.sync.send(Command::Move { folder: self.folder().to_string(), uid, to });
            }
            Do::SaveAttachment(i) => {
                let Some(a) = self.body().and_then(|b| b.attachments.get(i)).cloned() else { return };
                let Some(dir) = dirs::download_dir().or_else(dirs::home_dir) else { return };
                let path = unused_path(&dir, &a.name);
                self.status = Some(match std::fs::write(&path, &a.data) {
                    Ok(()) => Status::info(format!("Saved {}", path.display())),
                    Err(e) => Status::error(format!("Couldn't save {}: {e}", a.name)),
                });
            }
            Do::ShowText(on) => {
                self.config.show_text = on;
                self.config.save();
            }
            Do::Sort(order) => {
                self.config.order = order;
                self.config.save();
                order.sort(&mut self.messages);
                self.scroll_to_selected = true;
            }
            Do::Link(click) => match click.url.split_once(':').map(|(scheme, _)| scheme.to_ascii_lowercase()) {
                Some(s) if s == "mailto" => {
                    let to = click.url[7..].split('?').next().unwrap_or_default().to_string();
                    self.start_compose(Draft { to, ..Default::default() });
                }
                Some(s) if s == "http" || s == "https" => self.link = Some(click),
                _ => self.status = Some(Status::error(format!("Mail doesn't open links like {}", click.url))),
            },
            Do::LoadImages(urls) => {
                let Some(uid) = self.selected else { return };
                let key = (self.folder().to_string(), uid);
                if self.remote.key.as_ref() != Some(&key) {
                    self.remote = Remote { key: Some(key), ..Default::default() };
                }
                let r = &mut self.remote;
                let new: Vec<String> = urls
                    .into_iter()
                    .filter(|u| r.approved.insert(u.clone()) || r.failed.remove(u).is_some())
                    .filter(|u| !r.fetched.contains_key(u))
                    .collect();
                r.pending.extend(new.iter().cloned());
                r.version += 1;
                if !new.is_empty() {
                    self.fetcher.fetch(new);
                }
            }
            Do::ReviewImages => {
                if let Some(open) = &self.page {
                    // Remote state is reset whenever another message opens, so it's this one's.
                    let approved = &self.remote.approved;
                    self.review = Some(open.images.iter().map(|i| approved.contains(&i.url) || !i.flagged()).collect());
                }
            }
            Do::OpenLink(url) => {
                ctx.open_url(egui::OpenUrl::new_tab(url));
                self.link = None;
            }
            Do::CopyText(text) => {
                ctx.copy_text(text);
                self.link = None;
            }
            Do::Cut => ctx.send_viewport_cmd(egui::ViewportCommand::RequestCut),
            Do::Copy => ctx.send_viewport_cmd(egui::ViewportCommand::RequestCopy),
            Do::Paste => ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste),
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
                        Status::info(format!("Copied {} skins", export.copied.len()))
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

    fn set_flag(&mut self, flag: Flag, on: bool) {
        let Some(i) = self.index() else { return };
        let m = &mut self.messages[i];
        match flag {
            Flag::Seen => m.seen = on,
            Flag::Flagged => m.flagged = on,
        }
        let uid = m.uid;
        self.sync.send(Command::SetFlag { folder: self.folder().to_string(), uid, flag, on });
    }

    fn start_compose(&mut self, draft: Draft) {
        if self.compose.as_ref().is_some_and(|c| !c.draft.is_empty()) {
            self.status = Some(Status::error("Finish or discard the message you're writing first"));
            return;
        }
        self.compose = Some(Compose { draft, sending: false, error: None, focus_body: true });
    }

    /// Left side of the title bar: word menus (Patina's ADR-0006).
    fn menus(&self, ui: &mut egui::Ui, actions: &mut Vec<Do>) {
        fn item(ui: &mut egui::Ui, actions: &mut Vec<Do>, enabled: bool, label: &str, keys: &str, action: Do) {
            if ui.add_enabled(enabled, egui::Button::new(label).shortcut_text(keys)).clicked() {
                actions.push(action);
                ui.close();
            }
        }
        let connected = self.account.is_set();
        let open = self.body().is_some();
        let selected = self.selected.is_some();
        ui.menu_button("File", |ui| {
            item(ui, actions, connected, "New Message", "Ctrl+N", Do::New);
            item(ui, actions, connected, "Check for Mail", "F5", Do::Refresh);
            ui.separator();
            item(ui, actions, true, "Account…", "", Do::Account);
            ui.separator();
            item(ui, actions, true, "Quit", "Ctrl+Q", Do::Quit);
        });
        ui.menu_button("Edit", |ui| {
            item(ui, actions, true, "Cut", "Ctrl+X", Do::Cut);
            item(ui, actions, true, "Copy", "Ctrl+C", Do::Copy);
            item(ui, actions, true, "Paste", "Ctrl+V", Do::Paste);
        });
        ui.menu_button("View", |ui| {
            item(ui, actions, true, "Next Message", "↓", Do::Step(1));
            item(ui, actions, true, "Previous Message", "↑", Do::Step(-1));
            ui.separator();
            ui.menu_button("Sort By", |ui| self.sort_menu(ui, actions));
            ui.separator();
            let mut text = self.config.show_text;
            if ui.checkbox(&mut text, "Show HTML mail as text").changed() {
                actions.push(Do::ShowText(text));
            }
            let mut system = self.config.system_title_bar;
            if ui.checkbox(&mut system, "System title bar").changed() {
                actions.push(Do::SystemTitleBar(system));
            }
        });
        ui.menu_button("Message", |ui| {
            item(ui, actions, open, "Reply", "Ctrl+R", Do::Reply { all: false });
            item(ui, actions, open, "Reply All", "Ctrl+Shift+R", Do::Reply { all: true });
            item(ui, actions, open, "Forward", "Ctrl+L", Do::Forward);
            ui.separator();
            item(ui, actions, selected, "Mark as Unread", "Ctrl+U", Do::MarkUnread);
            item(ui, actions, selected, "Star", "S", Do::ToggleFlag);
            ui.separator();
            item(ui, actions, selected && self.find_folder("Archive").is_some(), "Archive", "A", Do::MoveTo("Archive"));
            item(
                ui,
                actions,
                selected && self.find_folder("Trash").is_some(),
                "Move to Trash",
                "Del",
                Do::MoveTo("Trash"),
            );
        });
        ui.menu_button("Skins", |ui| {
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
            let current = self.skin();
            for skin in &self.skins {
                let resp = ui.radio(skin.name == current.name, &skin.name);
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

    /// Sort By, in the View menu and over the list.
    fn sort_menu(&self, ui: &mut egui::Ui, actions: &mut Vec<Do>) {
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
        let order = self.config.order;
        for by in SortBy::ALL {
            if ui.radio(order.by == by, by.label()).clicked() {
                actions.push(Do::Sort(Order { by, reversed: false, ..order }));
            }
        }
        ui.separator();
        for (reversed, label) in [false, true].into_iter().zip(order.by.directions()) {
            if ui.radio(order.reversed == reversed, label).clicked() {
                actions.push(Do::Sort(Order { reversed, ..order }));
            }
        }
        ui.separator();
        let (mut unread, mut starred) = (order.unread_first, order.starred_first);
        if ui.checkbox(&mut unread, "Unread at the top").changed() {
            actions.push(Do::Sort(Order { unread_first: unread, ..order }));
        }
        if ui.checkbox(&mut starred, "Starred at the top").changed() {
            actions.push(Do::Sort(Order { starred_first: starred, ..order }));
        }
    }

    /// Right side of the title bar, laid out right to left: writing mail is
    /// what this app makes.
    fn output_actions(&self, ui: &mut egui::Ui, actions: &mut Vec<Do>) {
        let open = self.body().is_some();
        if ui.add_enabled(self.account.is_set(), egui::Button::new("New Message")).on_hover_text("Ctrl+N").clicked() {
            actions.push(Do::New);
        }
        ui.separator();
        if ui.add_enabled(open, egui::Button::new("Forward")).on_hover_text("Ctrl+L").clicked() {
            actions.push(Do::Forward);
        }
        if ui.add_enabled(open, egui::Button::new("Reply All")).on_hover_text("Ctrl+Shift+R").clicked() {
            actions.push(Do::Reply { all: true });
        }
        if ui.add_enabled(open, egui::Button::new("Reply")).on_hover_text("Ctrl+R").clicked() {
            actions.push(Do::Reply { all: false });
        }
    }

    fn shortcuts(&self, ctx: &egui::Context, actions: &mut Vec<Do>) {
        // Plain keys only while no text field has the keyboard.
        let typing = ctx.memory(|m| m.focused().is_some());
        ctx.input_mut(|i| {
            for (shortcut, action) in [
                (REPLY_ALL, Do::Reply { all: true }),
                (REPLY, Do::Reply { all: false }),
                (NEW, Do::New),
                (FORWARD, Do::Forward),
                (UNREAD, Do::MarkUnread),
                (QUIT, Do::Quit),
            ] {
                if i.consume_shortcut(&shortcut) {
                    actions.push(action);
                }
            }
            if i.consume_key(Modifiers::NONE, Key::F5) {
                actions.push(Do::Refresh);
            }
            if typing {
                return;
            }
            // Ctrl+C, and Edit → Copy, with text selected in an HTML message.
            if i.events.iter().any(|e| matches!(e, egui::Event::Copy))
                && let Some(text) = self.page.as_ref().and_then(|p| p.page.selected_text())
            {
                actions.push(Do::CopyText(text));
            }
            for (key, action) in [
                (Key::ArrowDown, Do::Step(1)),
                (Key::J, Do::Step(1)),
                (Key::ArrowUp, Do::Step(-1)),
                (Key::K, Do::Step(-1)),
                (Key::Delete, Do::MoveTo("Trash")),
                (Key::A, Do::MoveTo("Archive")),
                (Key::S, Do::ToggleFlag),
            ] {
                if i.consume_key(Modifiers::NONE, key) {
                    actions.push(action);
                }
            }
        });
    }

    fn sidebar(&self, ui: &mut egui::Ui, actions: &mut Vec<Do>) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.add_space(6.0);
            let mut heading: Option<&str> = None;
            for f in &self.folders {
                // Bridge's "Folders" and "Labels" can't be opened: show them as headings.
                let top = f.name.split('/').next().unwrap_or_default();
                if f.depth > 0 && heading != Some(top) && !self.folders.iter().any(|o| o.name == top) {
                    ui.add_space(8.0);
                    ui.label(RichText::new(top).weak().small());
                }
                heading = Some(top);
                if folder_row(ui, f, f.name == self.config.folder).clicked() {
                    actions.push(Do::Open(f.name.clone()));
                }
            }
            if self.folders.is_empty() && !self.sync.busy {
                ui.add_space(12.0);
                ui.label(RichText::new("No folders yet").weak());
            }
        });
    }

    fn message_list(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Do>) {
        self.sort_strip(ui, actions);
        if self.messages.is_empty() {
            ui.centered_and_justified(|ui| {
                let text = if self.listed.is_none() && self.account.is_set() { "Loading…" } else { "No messages" };
                ui.label(RichText::new(text).weak());
            });
            return;
        }
        let row = 52.0;
        let scroll_to = self.scroll_to_selected.then(|| self.index()).flatten();
        self.scroll_to_selected = false;
        let mut area = egui::ScrollArea::vertical().auto_shrink(false);
        if let Some(i) = scroll_to {
            // Keep the selection in view, as arrow keys move it.
            let top = i as f32 * row;
            let view = ui.available_height();
            let offset = ui.ctx().data(|d| d.get_temp::<f32>(egui::Id::new("list-offset"))).unwrap_or(0.0);
            if top < offset {
                area = area.vertical_scroll_offset(top);
            } else if top + row > offset + view {
                area = area.vertical_scroll_offset(top + row - view);
            }
        }
        let output = area.show_rows(ui, row, self.messages.len(), |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for m in &self.messages[range] {
                if message_row(ui, m, Some(m.uid) == self.selected, row).clicked() {
                    actions.push(Do::Select(m.uid));
                }
            }
        });
        ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new("list-offset"), output.state.offset.y));
    }

    /// The strip over the list saying how it's sorted, which opens Sort By.
    fn sort_strip(&self, ui: &mut egui::Ui, actions: &mut Vec<Do>) {
        let order = self.config.order;
        let direction = order.by.directions()[order.reversed as usize];
        let text = match order.by {
            SortBy::Date => direction.to_string(),
            by => format!("{}, {direction}", by.label()),
        };
        egui::Frame::new().inner_margin(egui::Margin { left: 12, right: 6, top: 2, bottom: 2 }).show(ui, |ui| {
            ui.horizontal(|ui| {
                if order.starred_first || order.unread_first {
                    let tops = match (order.starred_first, order.unread_first) {
                        (true, true) => "Starred and unread at the top",
                        (true, false) => "Starred at the top",
                        _ => "Unread at the top",
                    };
                    ui.label(RichText::new(tops).small().weak());
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.menu_button(RichText::new(format!("{text} ⏷")).small(), |ui| self.sort_menu(ui, actions))
                        .response
                        .on_hover_text("Sort the list");
                });
            });
        });
        ui.separator();
    }

    fn reader(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Do>) {
        // Out of `self` while drawing, since the open message is borrowed from it.
        let mut page = self.page.take();
        self.draw_reader(ui, actions, &mut page);
        self.page = page;
    }

    fn draw_reader(&self, ui: &mut egui::Ui, actions: &mut Vec<Do>, page: &mut Option<OpenPage>) {
        let Some(uid) = self.selected else {
            *page = None;
            ui.centered_and_justified(|ui| ui.label(RichText::new("No message selected").weak()));
            return;
        };
        let Some(body) = self.body() else {
            ui.centered_and_justified(|ui| ui.spinner());
            return;
        };
        let flagged = self.messages.iter().any(|m| m.uid == uid && m.flagged);
        egui::Frame::new().inner_margin(egui::Margin { left: 18, right: 18, top: 14, bottom: 6 }).show(ui, |ui| {
            ui.horizontal(|ui| {
                let subject = if body.subject.is_empty() { "(no subject)" } else { &body.subject };
                ui.add(egui::Label::new(RichText::new(subject).heading()).wrap());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let star = if flagged { "★" } else { "☆" };
                    if ui.button(star).on_hover_text("Star (S)").clicked() {
                        actions.push(Do::ToggleFlag);
                    }
                });
            });
            ui.add_space(4.0);
            egui::Grid::new("headers").num_columns(2).spacing([8.0, 2.0]).show(ui, |ui| {
                let mut line = |label: &str, value: String| {
                    if !value.is_empty() {
                        ui.label(RichText::new(label).weak());
                        ui.add(egui::Label::new(value).wrap().selectable(true));
                        ui.end_row();
                    }
                };
                line("From", message::join(&body.from));
                line("To", message::join(&body.to));
                line("Cc", message::join(&body.cc));
                line("Date", long_date(body.date));
            });
            if !body.attachments.is_empty() {
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    for (i, a) in body.attachments.iter().enumerate() {
                        let label = format!("📎 {} ({})", a.name, size(a.data.len()));
                        if ui.button(label).on_hover_text("Save to Downloads").clicked() {
                            actions.push(Do::SaveAttachment(i));
                        }
                    }
                });
            }
        });

        let html = body.html.as_deref().filter(|_| !self.config.show_text);
        let Some(html) = html else {
            *page = None;
            ui.separator();
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                egui::Frame::new().inner_margin(egui::Margin::symmetric(18, 6)).show(ui, |ui| {
                    ui.add(egui::Label::new(body.text.trim_end()).wrap().selectable(true));
                    ui.add_space(12.0);
                });
            });
            return;
        };
        let folder = self.folder();
        let remote = self.remote.key.as_ref().is_some_and(|(f, u)| f == folder && *u == uid).then_some(&self.remote);
        let version = remote.map_or(0, |r| r.version);
        if !page.as_ref().is_some_and(|p| p.folder == folder && p.uid == uid && p.version == version) {
            let empty = (HashSet::new(), HashMap::new());
            let (approved, fetched) = remote.map_or((&empty.0, &empty.1), |r| (&r.approved, &r.fetched));
            let (prepared, images) = remote::prepare(html, approved);
            let p = Page::new(
                &prepared,
                body.parts.clone(),
                fetched.clone(),
                &images,
                ui.available_width(),
                ui.ctx().pixels_per_point(),
            );
            *page = Some(OpenPage { folder: folder.to_string(), uid, version, page: p, images });
        }
        let Some(open) = page else { return };
        self.remote_bar(ui, open, remote, actions);
        let width = ui.available_width();
        egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| match open.page.show(ui, width) {
            Some(Event::Link(click)) => actions.push(Do::Link(click)),
            Some(Event::LoadImage(i)) => {
                if let Some(image) = open.images.get(i) {
                    actions.push(Do::LoadImages(vec![image.url.clone()]));
                }
            }
            None => {}
        });
    }

    /// Under the headers: what's blocked, what's loading, and Review….
    fn remote_bar(&self, ui: &mut egui::Ui, open: &OpenPage, remote: Option<&Remote>, actions: &mut Vec<Do>) {
        let approved = |url: &str| remote.is_some_and(|r| r.approved.contains(url));
        let blocked: Vec<&RemoteImage> = open.images.iter().filter(|i| !approved(&i.url)).collect();
        let pixels = blocked.iter().filter(|i| i.suspected_pixel()).count();
        let image_urls: HashSet<&str> = open.images.iter().map(|i| i.url.as_str()).collect();
        let other: Vec<String> = open.page.blocked().into_iter().filter(|u| !image_urls.contains(u.as_str())).collect();
        let loading = remote.map_or(0, |r| r.pending.len());
        let failed = remote.map_or(0, |r| r.failed.len());
        let zoom = open.page.zoom();
        if open.images.is_empty() && other.is_empty() && zoom >= 1.0 {
            return;
        }
        egui::Frame::new().inner_margin(egui::Margin { left: 18, right: 18, top: 0, bottom: 6 }).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                let small = |text: String| RichText::new(text).weak().small();
                if !blocked.is_empty() {
                    let n = blocked.len();
                    let mut text = format!("{n} remote image{} blocked", if n == 1 { "" } else { "s" });
                    if pixels > 0 {
                        text += &format!(", {pixels} suspected tracking pixel{}", if pixels == 1 { "" } else { "s" });
                    }
                    ui.label(small(text));
                }
                if !open.images.is_empty() && ui.small_button("Review…").clicked() {
                    actions.push(Do::ReviewImages);
                }
                if loading > 0 {
                    ui.spinner();
                    ui.label(small(format!("Loading {loading}…")));
                }
                if failed > 0 {
                    let reasons: Vec<String> =
                        remote.into_iter().flat_map(|r| r.failed.iter().map(|(u, e)| format!("{u}: {e}"))).collect();
                    ui.label(RichText::new(format!("{failed} didn't load")).small().color(ui.visuals().warn_fg_color))
                        .on_hover_text(reasons.join("\n"));
                }
                if !other.is_empty() {
                    let n = other.len();
                    ui.label(small(format!("· {n} other remote item{} blocked", if n == 1 { "" } else { "s" })))
                        .on_hover_text(format!("Styles, fonts and backgrounds never load:\n{}", other.join("\n")));
                }
                if zoom < 1.0 {
                    ui.label(small(format!("· shown at {:.0}% to fit", zoom * 100.0)));
                }
            });
        });
    }

    /// The Review… list: every remote image, what it would contact, and flags.
    fn review_window(&mut self, ctx: &egui::Context, actions: &mut Vec<Do>) {
        let (Some(ticks), Some(open)) = (&mut self.review, &self.page) else { return };
        let remote =
            self.remote.key.as_ref().is_some_and(|(f, u)| *f == open.folder && *u == open.uid).then_some(&self.remote);
        let mut close = false;
        let modal = egui::Modal::new(egui::Id::new("images-dialog")).show(ctx, |ui| {
            ui.set_width(520.0);
            ui.heading("Remote images");
            ui.label(
                RichText::new(
                    "Loading an image tells its server that you opened this message, when, and from which IP address. \
Flags help spot trackers, but an image without flags can still track you.",
                )
                .weak(),
            );
            ui.add_space(6.0);
            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                for (i, image) in open.images.iter().enumerate() {
                    let loaded = remote.is_some_and(|r| r.fetched.contains_key(&image.url));
                    let pending = remote.is_some_and(|r| r.pending.contains(&image.url));
                    let failed = remote.and_then(|r| r.failed.get(&image.url));
                    ui.horizontal(|ui| {
                        let mut on = ticks[i] || loaded;
                        ui.add_enabled_ui(!loaded && !pending, |ui| {
                            if ui.checkbox(&mut on, "").changed() {
                                ticks[i] = on;
                            }
                        });
                        ui.vertical(|ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(RichText::new(&image.host).strong()).on_hover_text(&image.url);
                                ui.label(RichText::new(image.size_text()).weak().small());
                                let warn = ui.visuals().warn_fg_color;
                                let mut flag = |text: &str| ui.label(RichText::new(text).small().color(warn));
                                if image.width.is_some_and(|w| w <= 4.0) || image.height.is_some_and(|h| h <= 4.0) {
                                    flag("tiny");
                                }
                                if image.hidden {
                                    flag("hidden");
                                }
                                if image.trackerish {
                                    flag("looks like a tracker");
                                }
                                if loaded {
                                    ui.label(RichText::new("loaded").small().weak());
                                } else if pending {
                                    ui.spinner();
                                } else if let Some(e) = failed {
                                    ui.label(RichText::new("didn't load").small().color(ui.visuals().error_fg_color))
                                        .on_hover_text(e);
                                }
                            });
                            ui.add(egui::Label::new(RichText::new(&image.url).weak().small()).truncate())
                                .on_hover_text(&image.url);
                        });
                    });
                    ui.add_space(4.0);
                }
            });
            ui.add_space(6.0);
            let chosen: Vec<String> = open
                .images
                .iter()
                .zip(ticks.iter())
                .filter(|(image, on)| **on && !remote.is_some_and(|r| r.approved.contains(&image.url)))
                .map(|(image, _)| image.url.clone())
                .collect();
            ui.horizontal(|ui| {
                let label = format!("Load {} selected", chosen.len());
                if ui.add_enabled(!chosen.is_empty(), egui::Button::new(label)).clicked() {
                    actions.push(Do::LoadImages(chosen.clone()));
                    close = true;
                }
                if ui.button("Select all unflagged").clicked() {
                    for (on, image) in ticks.iter_mut().zip(&open.images) {
                        *on = !image.flagged();
                    }
                }
                if ui.button("Cancel").clicked() {
                    close = true;
                }
            });
        });
        if close || modal.should_close() {
            self.review = None;
        }
    }

    fn link_window(&mut self, ctx: &egui::Context, actions: &mut Vec<Do>) {
        let Some(link) = &self.link else { return };
        let mut close = false;
        let modal = egui::Modal::new(egui::Id::new("link-dialog")).show(ctx, |ui| {
            ui.set_max_width(460.0);
            ui.heading("Open this link?");
            ui.add_space(4.0);
            ui.add(egui::Label::new(RichText::new(&link.url).monospace()).wrap().selectable(true));
            if let Some(claimed) = html::misleading(&link.text, &link.url) {
                ui.add_space(6.0);
                let real = link.url.split("://").nth(1).and_then(|r| r.split('/').next()).unwrap_or(&link.url);
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    format!("Careful: the link's text says {claimed}, but it goes to {real}."),
                );
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Open in Browser").clicked() {
                    actions.push(Do::OpenLink(link.url.clone()));
                }
                if ui.button("Copy Address").clicked() {
                    actions.push(Do::CopyText(link.url.clone()));
                }
                if ui.button("Cancel").clicked() {
                    close = true;
                }
            });
        });
        if close || modal.should_close() {
            self.link = None;
        }
    }

    fn status_bar(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Do>) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("v{}", env!("CARGO_PKG_VERSION"))).weak().small());
            let skin_errors = (!self.skin_errors.is_empty())
                .then(|| Status::error(format!("Couldn't load {} skin(s)", self.skin_errors.len())));
            if let Some(status) = skin_errors.as_ref().or(self.status.as_ref()) {
                ui.separator();
                let color = if status.error { ui.visuals().error_fg_color } else { ui.visuals().weak_text_color() };
                let resp = ui.label(RichText::new(&status.text).color(color).small());
                if skin_errors.is_some() {
                    resp.on_hover_text(self.skin_errors.join("\n"));
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.sync.busy {
                    ui.label(RichText::new("Checking…").weak().small());
                    ui.spinner();
                } else if !self.account.is_set() {
                    if ui.small_button("Set up account…").clicked() {
                        actions.push(Do::Account);
                    }
                } else {
                    let n = self.messages.len();
                    ui.label(RichText::new(format!("{n} message{}", if n == 1 { "" } else { "s" })).weak().small())
                        .on_hover_text(format!("{} on {}", self.account.username, self.account.host));
                }
            });
        });
    }

    fn compose_window(&mut self, ctx: &egui::Context, actions: &mut Vec<Do>) {
        let Some(c) = &mut self.compose else { return };
        // The window's size isn't known on the first frames, so it would centre wrongly.
        if ctx.cumulative_frame_nr() < 2 {
            ctx.request_repaint();
            return;
        }
        let mut open = true;
        let (mut send, mut discard) = (false, false);
        let title = if c.draft.subject.trim().is_empty() { "New Message".to_string() } else { c.draft.subject.clone() };
        egui::Window::new(title)
            .id(egui::Id::new("compose"))
            .open(&mut open)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .default_size([640.0, 480.0])
            .min_width(420.0)
            .collapsible(false)
            .resizable(true)
            .show(ctx, |ui| {
                ui.add_enabled_ui(!c.sending, |ui| {
                    // Rows rather than a grid, so the fields can take the window's width.
                    for (label, text) in
                        [("To", &mut c.draft.to), ("Cc", &mut c.draft.cc), ("Subject", &mut c.draft.subject)]
                    {
                        ui.horizontal(|ui| {
                            ui.add_sized([56.0, 20.0], egui::Label::new(label));
                            ui.add(egui::TextEdit::singleline(text).desired_width(f32::INFINITY));
                        });
                    }
                    ui.add_space(4.0);
                    let bottom = 34.0;
                    egui::ScrollArea::vertical().max_height(ui.available_height() - bottom).show(ui, |ui| {
                        let body = ui.add_sized(
                            [ui.available_width(), (ui.available_height() - bottom).max(160.0)],
                            egui::TextEdit::multiline(&mut c.draft.text).id(egui::Id::new("compose-body")),
                        );
                        if c.focus_body {
                            c.focus_body = false;
                            body.request_focus();
                            // Replies start above the quote.
                            if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), body.id) {
                                let start = egui::text::CCursor::new(0);
                                state.cursor.set_char_range(Some(egui::text::CCursorRange::one(start)));
                                state.store(ui.ctx(), body.id);
                            }
                        }
                        body.context_menu(|ui| {
                            for (label, keys, action) in [
                                ("Cut", "Ctrl+X", Do::Cut),
                                ("Copy", "Ctrl+C", Do::Copy),
                                ("Paste", "Ctrl+V", Do::Paste),
                            ] {
                                if ui.add(egui::Button::new(label).shortcut_text(keys)).clicked() {
                                    // The menu took the keyboard: give it back so the edit lands here.
                                    ui.ctx().memory_mut(|m| m.request_focus(body.id));
                                    actions.push(action);
                                    ui.close();
                                }
                            }
                        });
                    });
                    ui.horizontal(|ui| {
                        if ui.button("Send").on_hover_text("Ctrl+Enter").clicked() {
                            send = true;
                        }
                        if ui.button("Discard").clicked() {
                            discard = true;
                        }
                        if c.sending {
                            ui.spinner();
                            ui.label(RichText::new("Sending…").weak());
                        }
                        if let Some(e) = &c.error {
                            ui.colored_label(ui.visuals().error_fg_color, e);
                        }
                    });
                });
            });
        if ctx.input_mut(|i| i.consume_shortcut(&SEND)) && !c.sending {
            send = true;
        }
        if send {
            match c.draft.build(&self.account) {
                Ok(m) => {
                    c.sending = true;
                    c.error = None;
                    self.sync.send_mail(self.account.clone(), m);
                }
                Err(e) => c.error = Some(e),
            }
        }
        if (!open || discard) && !c.sending {
            self.compose = None;
        }
    }

    fn account_window(&mut self, ctx: &egui::Context) {
        let Some(account) = &mut self.account_dialog else { return };
        let (mut save, mut cancel) = (false, false);
        let modal = egui::Modal::new(egui::Id::new("account-dialog")).show(ctx, |ui| {
            ui.set_width(400.0);
            ui.heading("Account");
            ui.label(
                RichText::new("Your Proton Mail Bridge login. Bridge shows these under the account's mailbox details.")
                    .weak(),
            );
            ui.add_space(6.0);
            egui::Grid::new("account-fields").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                ui.label("Name");
                ui.add(egui::TextEdit::singleline(&mut account.name).hint_text("Shown to people you write to"));
                ui.end_row();
                ui.label("Username");
                ui.add(egui::TextEdit::singleline(&mut account.username).hint_text("you@proton.me"));
                ui.end_row();
                ui.label("Password");
                ui.add(egui::TextEdit::singleline(&mut account.password).password(true).hint_text("Bridge's password"));
                ui.end_row();
                ui.label("Server");
                ui.add(egui::TextEdit::singleline(&mut account.host));
                ui.end_row();
                ui.label("IMAP port");
                ui.add(egui::DragValue::new(&mut account.imap_port).range(1..=65535));
                ui.end_row();
                ui.label("SMTP port");
                ui.add(egui::DragValue::new(&mut account.smtp_port).range(1..=65535));
                ui.end_row();
                ui.label("Security");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut account.security, Security::StartTls, "STARTTLS");
                    ui.radio_value(&mut account.security, Security::Ssl, "SSL");
                });
                ui.end_row();
            });
            ui.label(RichText::new("Saved in ~/.config/mail/account.toml, readable only by you.").weak().small());
            if !account.is_local() {
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    "Bridge's certificate is only trusted on this machine; another server needs a valid one.",
                );
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.add_enabled(account.is_set(), egui::Button::new("Connect")).clicked() {
                    save = true;
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        });
        if save {
            let account = account.clone();
            match config::save_account(&account) {
                Ok(()) => {
                    self.account = account.clone();
                    self.folders.clear();
                    self.messages.clear();
                    self.bodies.clear();
                    self.page = None;
                    self.selected = None;
                    self.listed = None;
                    self.sync.send(Command::SetAccount(account));
                    self.sync.send(Command::Refresh);
                    self.sync.send(Command::Open(self.config.folder.clone()));
                    self.account_dialog = None;
                }
                Err(e) => self.status = Some(Status::error(format!("Couldn't save the account: {e}"))),
            }
        } else if cancel || modal.should_close() {
            self.account_dialog = None;
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

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll();
        let now = ctx.input(|i| i.time);
        if self.account.is_set() && self.screenshot.is_none() && now - self.last_refresh > REFRESH_EVERY {
            if self.last_refresh > 0.0 {
                self.sync.send(Command::Refresh);
            }
            self.last_refresh = now;
        }
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(REFRESH_EVERY));

        let custom = !self.config.system_title_bar;
        let style = self.skin().frame_style(ui.visuals());
        let radius = if custom { frame::corner_radius(&ctx, style.radius) } else { 0 };

        let (mut actions, mut output) = (Vec::new(), Vec::new());
        egui::Panel::top("header").frame(frame::header_frame(ui.style(), &style, radius)).show(ui, |ui| {
            TitleBar::new("Mail", &style).custom(custom).menu_bar(true).span(frame::header_span(ui)).show(
                ui,
                |ui| self.menus(ui, &mut actions),
                |ui| self.output_actions(ui, &mut output),
            );
        });
        actions.append(&mut output);
        egui::Panel::bottom("status")
            .frame(
                egui::Frame::side_top_panel(ui.style())
                    .inner_margin(egui::Margin::symmetric(10, 4))
                    .corner_radius(egui::CornerRadius { sw: radius, se: radius, ..Default::default() }),
            )
            .show(ui, |ui| self.status_bar(ui, &mut actions));
        // Never narrower than the longest folder name: a saved width from before
        // can't cut names off, and a new default wouldn't reach a saved window.
        let fit = folders_width(ui, &self.folders);
        egui::Panel::left("folders")
            .frame(egui::Frame::side_top_panel(ui.style()))
            .default_size(fit.max(240.0))
            .min_size(fit)
            .resizable(true)
            .show(ui, |ui| self.sidebar(ui, &mut actions));
        egui::Panel::left("messages")
            .frame(egui::Frame::central_panel(ui.style()).inner_margin(0))
            .default_size(380.0)
            .resizable(true)
            .show(ui, |ui| self.message_list(ui, &mut actions));
        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(ui.style()).inner_margin(0))
            .show(ui, |ui| self.reader(ui, &mut actions));

        self.compose_window(&ctx, &mut actions);
        self.account_window(&ctx);
        self.link_window(&ctx, &mut actions);
        self.review_window(&ctx, &mut actions);
        if self.account_dialog.is_none() && self.link.is_none() && self.review.is_none() {
            self.shortcuts(&ctx, &mut actions);
        }
        if self.demo_review && self.page.is_some() {
            self.demo_review = false;
            actions.push(Do::ReviewImages);
        }
        for action in actions {
            self.act(&ctx, action);
        }

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

/// The sidebar width that shows every folder's name and unread count in
/// full, as `folder_row` lays them out, within reason.
fn folders_width(ui: &egui::Ui, folders: &[Folder]) -> f32 {
    let body = egui::TextStyle::Body.resolve(ui.style());
    let small = egui::TextStyle::Small.resolve(ui.style());
    let text_width = |text: String, font: &egui::FontId| {
        ui.fonts_mut(|f| f.layout_no_wrap(text, font.clone(), egui::Color32::WHITE).size().x)
    };
    let widest = folders
        .iter()
        .map(|f| {
            let count = if f.unseen > 0 { 12.0 + text_width(f.unseen.to_string(), &small) } else { 0.0 };
            10.0 + 14.0 * f.depth as f32 + text_width(f.label.clone(), &body) + count + 10.0
        })
        .fold(0.0, f32::max);
    // The panel's own margins and the scroll bar's room.
    let margins = egui::Frame::side_top_panel(ui.style()).total_margin().sum().x + 8.0;
    (widest + margins).clamp(160.0, 360.0)
}

/// A folder in the sidebar, with its unread count.
fn folder_row(ui: &mut egui::Ui, f: &Folder, selected: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 26.0), egui::Sense::click());
    let v = ui.visuals();
    if selected {
        ui.painter().rect_filled(rect, 4.0, v.selection.bg_fill);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, 4.0, v.widgets.hovered.weak_bg_fill);
    }
    let color = if selected { v.selection.stroke.color } else { v.text_color() };
    let x = rect.left() + 10.0 + 14.0 * f.depth as f32;
    let font = egui::TextStyle::Body.resolve(ui.style());
    let strong = if f.unseen > 0 { v.strong_text_color() } else { color };
    ui.painter().text(
        egui::pos2(x, rect.center().y),
        egui::Align2::LEFT_CENTER,
        &f.label,
        font.clone(),
        if selected { color } else { strong },
    );
    if f.unseen > 0 {
        let count = egui::TextStyle::Small.resolve(ui.style());
        ui.painter().text(
            egui::pos2(rect.right() - 10.0, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            f.unseen.to_string(),
            count,
            if selected { color } else { v.weak_text_color() },
        );
    }
    resp
}

/// A message in the list: who it's from and when, the subject below.
fn message_row(ui: &mut egui::Ui, m: &Summary, selected: bool, height: f32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), height), egui::Sense::click());
    let v = ui.visuals();
    if selected {
        ui.painter().rect_filled(rect, 0.0, v.selection.bg_fill);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, 0.0, v.widgets.hovered.weak_bg_fill);
    }
    let (text, weak) = if selected {
        (v.selection.stroke.color, v.selection.stroke.color.gamma_multiply(0.8))
    } else {
        (if m.seen { v.text_color() } else { v.strong_text_color() }, v.weak_text_color())
    };
    let painter = ui.painter_at(rect.shrink2(egui::vec2(2.0, 0.0)));
    let body = egui::TextStyle::Body.resolve(ui.style());
    let small = egui::TextStyle::Small.resolve(ui.style());
    let left = rect.left() + 22.0;
    let top = rect.top() + 8.0;
    if !m.seen {
        let dot = if selected { text } else { v.hyperlink_color };
        painter.circle_filled(egui::pos2(rect.left() + 11.0, top + 8.0), 3.5, dot);
    }
    let date =
        painter.text(egui::pos2(rect.right() - 12.0, top), egui::Align2::RIGHT_TOP, short_date(m.date), small, weak);
    let from = m.from.as_ref().map(|a| a.short().to_string()).unwrap_or_else(|| "(unknown)".into());
    let star = if m.flagged { "★ " } else { "" };
    let galley = ui.fonts_mut(|f| f.layout_no_wrap(format!("{star}{from}"), body.clone(), text));
    let clip = egui::Rect::from_min_max(egui::pos2(left, top), egui::pos2(date.left() - 8.0, top + 20.0));
    ui.painter_at(clip).galley(clip.min, galley, text);
    let subject = if m.subject.is_empty() { "(no subject)" } else { &m.subject };
    let clip = egui::Rect::from_min_max(egui::pos2(left, top + 20.0), egui::pos2(rect.right() - 12.0, rect.bottom()));
    let galley = ui.fonts_mut(|f| f.layout_no_wrap(subject.to_string(), body, weak));
    ui.painter_at(clip).galley(clip.min, galley, weak);
    let line = egui::Stroke::new(1.0, v.widgets.noninteractive.bg_stroke.color.gamma_multiply(0.5));
    ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, line);
    resp
}

/// The list's date: the time today, the day this year, else the full date.
fn short_date(date: i64) -> String {
    let Some(d) = chrono::DateTime::from_timestamp(date, 0).map(|d| d.with_timezone(&chrono::Local)) else {
        return String::new();
    };
    let now = chrono::Local::now();
    if d.date_naive() == now.date_naive() {
        d.format("%H:%M").to_string()
    } else if d.format("%Y").to_string() == now.format("%Y").to_string() {
        d.format("%-d %b").to_string()
    } else {
        d.format("%-d %b %Y").to_string()
    }
}

fn long_date(date: i64) -> String {
    chrono::DateTime::from_timestamp(date, 0)
        .map(|d| d.with_timezone(&chrono::Local).format("%A, %-d %B %Y at %H:%M").to_string())
        .unwrap_or_default()
}

fn size(bytes: usize) -> String {
    match bytes {
        b if b >= 1 << 20 => format!("{:.1} MB", b as f64 / (1 << 20) as f64),
        b if b >= 1 << 10 => format!("{} KB", b >> 10),
        b => format!("{b} B"),
    }
}

/// `dir/name`, or `dir/name (2)` and so on if that's taken. Strips any path
/// in the name, since it comes from the sender.
fn unused_path(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    let name = std::path::Path::new(name).file_name().and_then(|n| n.to_str()).unwrap_or("attachment");
    let path = dir.join(name);
    if !path.exists() {
        return path;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s, format!(".{e}")),
        _ => (name, String::new()),
    };
    (2..).map(|n| dir.join(format!("{stem} ({n}){ext}"))).find(|p| !p.exists()).expect("a free name")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_skin_is_built_in() {
        let (skins, _) = skin::load_all(skin::BUILT_IN, &[]);
        assert!(skins.iter().any(|s| s.name == HOME_SKIN));
    }

    #[test]
    fn attachment_names_never_leave_the_folder() {
        let dir = std::env::temp_dir().join(format!("mail-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(unused_path(&dir, "../../etc/passwd"), dir.join("passwd"));
        std::fs::write(dir.join("a.txt"), "").unwrap();
        assert_eq!(unused_path(&dir, "a.txt"), dir.join("a (2).txt"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn sizes_read_well() {
        assert_eq!((size(12), size(2048), size(3 << 20)), ("12 B".into(), "2 KB".into(), "3.0 MB".into()));
    }
}
