//! The window: Patina's frame and title bar with the sheets as tabs, the
//! formula bar and grid, and a status bar.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};

use egui::{Key, Modifiers};
use patina::frame::{self, TitleBar};
use patina::skin::{self, Skin};
use patina::tabs::{self, TabAction, TabStrip};

use crate::config::Config;
use crate::formula::{self, Pos, cell_name};
use crate::grid::{self, View};
use crate::sheet::{Sheet, is_shell};
use crate::shell::{self, Status};
use crate::store::{self, Document, Write};

/// The skin used until the user picks one: stock egui, following the desktop.
const HOME_SKIN: &str = "Default";
const EXTENSION: &str = "sqlite3";

/// What the title bar's menus ask for, handled after they're drawn.
enum Action {
    New,
    Open,
    OpenPath(PathBuf),
    Close,
    Quit,
    Trust(bool),
    RunShell,
    NewSheet,
    Rename(usize),
    Delete(usize),
    MoveSheet(usize, usize),
    Cut,
    Copy,
    Paste,
    Clear,
    SetSkin(String),
    SystemTitleBar(bool),
}

enum Dialog {
    Rename { index: usize, text: String, fresh: bool },
    Delete { index: usize },
}

#[derive(Clone, Copy, PartialEq)]
enum Pick {
    New,
    Open,
}

/// A shell command that finished, on its way from its thread to the sheet.
struct Finished {
    sheet: i64,
    pos: Pos,
    run: shell::Run,
}

/// Screenshot mode: where to save, and whether it has been asked for.
struct Screenshot {
    path: PathBuf,
    requested: bool,
}

pub struct App {
    config: Config,
    custom_frame: bool,
    skins: Vec<Skin>,
    skin_errors: Vec<String>,
    forced_skin: Option<String>,
    doc: Option<Document>,
    active: usize,
    views: HashMap<i64, View>,
    dialog: Option<Dialog>,
    /// A file dialog is open in its own thread.
    picking: Option<(Pick, Receiver<Option<PathBuf>>)>,
    /// The last thing that went wrong, shown in the status bar.
    problem: Option<String>,
    title: String,
    screenshot: Option<Screenshot>,
    finished_tx: Sender<Finished>,
    finished_rx: Receiver<Finished>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, config: Config, open: Option<PathBuf>) -> Self {
        patina::appearance::watch(&cc.egui_ctx);
        patina::emoji::install(patina_emoji_font::FONT);
        let dirs: Vec<_> = skin::app_dir("ledger").into_iter().collect();
        let (skins, skin_errors) = skin::load_all(skin::BUILT_IN, &dirs);
        let (finished_tx, finished_rx) = mpsc::channel();
        let mut app = Self {
            finished_tx,
            finished_rx,
            custom_frame: !config.system_title_bar,
            config,
            skins,
            skin_errors,
            forced_skin: std::env::var("LEDGER_SKIN").ok(),
            doc: None,
            active: 0,
            views: HashMap::new(),
            dialog: None,
            picking: None,
            problem: None,
            title: String::new(),
            screenshot: std::env::var_os("LEDGER_SCREENSHOT").map(|p| Screenshot { path: p.into(), requested: false }),
        };
        app.apply_skin(&cc.egui_ctx);
        if let Some(demo) = std::env::var_os("LEDGER_DEMO").map(PathBuf::from) {
            match crate::demo::make(&demo) {
                Ok(()) => app.open_path(&demo),
                Err(e) => app.problem = Some(e),
            }
        } else if let Some(path) = open.or_else(|| app.config.recent.first().cloned()) {
            if path.is_file() {
                app.open_path(&path);
            } else {
                app.config.forget(&path);
            }
        }
        // For screenshots: run the shell cells (in a trusted ledger) once it's open.
        if std::env::var_os("LEDGER_RUN_SHELL").is_some() {
            app.run_all_shell(&cc.egui_ctx);
        }
        // For screenshots: start on this sheet.
        if let (Some(name), Some(doc)) = (std::env::var("LEDGER_SHEET").ok(), &app.doc) {
            app.active = doc.sheets.iter().position(|s| s.name == name).unwrap_or(0);
        }
        app
    }

    /// The chosen skin, falling back to `HOME_SKIN` (a saved name may no
    /// longer exist), then Default.
    fn skin(&self) -> &Skin {
        let find = |name: &str| self.skins.iter().find(|s| s.name.eq_ignore_ascii_case(name));
        let wanted = self.forced_skin.as_deref().or(self.config.skin.as_deref()).unwrap_or(HOME_SKIN);
        find(wanted).or_else(|| find(HOME_SKIN)).unwrap_or(&self.skins[0])
    }

    fn apply_skin(&self, ctx: &egui::Context) {
        // Replaces the whole egui style: re-apply any spacing of the app's own after it.
        self.skin().apply(ctx);
    }

    // --- Files ---------------------------------------------------------

    fn open_path(&mut self, path: &Path) {
        self.finish_edit();
        match store::open(path) {
            Ok(doc) => self.show_document(doc),
            Err(e) => {
                self.config.forget(path);
                self.problem = Some(e);
            }
        }
    }

    fn new_path(&mut self, path: &Path) {
        self.finish_edit();
        match store::create(path) {
            Ok(doc) => self.show_document(doc),
            Err(e) => self.problem = Some(e),
        }
    }

    fn show_document(&mut self, doc: Document) {
        self.config.used(&doc.path);
        self.doc = Some(doc);
        self.active = 0;
        self.views.clear();
        self.problem = None;
    }

    fn close_document(&mut self) {
        self.finish_edit();
        self.doc = None; // dropping the writer finishes the saves
        self.views.clear();
        self.dialog = None;
    }

    /// Opens a file dialog.
    fn pick(&mut self, ctx: &egui::Context, kind: Pick) {
        if self.picking.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        spawn_dialog(ctx.clone(), kind, tx);
        self.picking = Some((kind, rx));
    }

    fn poll_picker(&mut self) {
        let Some((kind, rx)) = &self.picking else { return };
        match rx.try_recv() {
            Ok(path) => {
                let kind = *kind;
                self.picking = None;
                if let Some(mut path) = path {
                    match kind {
                        Pick::Open => self.open_path(&path),
                        Pick::New => {
                            if path.extension().is_none() {
                                path.set_extension(EXTENSION);
                            }
                            self.new_path(&path);
                        }
                    }
                }
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => self.picking = None,
        }
    }

    // --- Shell formulas (ADR-0006) -------------------------------------

    fn trusted(&self) -> bool {
        self.doc.as_ref().is_some_and(|d| self.config.is_trusted(&d.path))
    }

    /// Runs the shell formula in a cell, on a thread. Does nothing in a ledger
    /// that isn't trusted, or for a cell that's already running.
    fn run_shell(&mut self, ctx: &egui::Context, index: usize, pos: Pos) {
        if !self.trusted() {
            return;
        }
        let Some(doc) = &mut self.doc else { return };
        let sheet = &mut doc.sheets[index];
        if sheet.is_pending(pos) {
            return;
        }
        match sheet.shell_job(pos) {
            Ok(job) => {
                sheet.set_pending(pos, true);
                let (id, tx, ctx) = (sheet.id, self.finished_tx.clone(), ctx.clone());
                std::thread::spawn(move || {
                    let _ = tx.send(Finished { sheet: id, pos, run: shell::run(&job) });
                    ctx.request_repaint();
                });
            }
            Err(e) => self.problem = Some(format!("{}: {e}", cell_name(pos))),
        }
    }

    fn run_all_shell(&mut self, ctx: &egui::Context) {
        self.finish_edit();
        let cells: Vec<(usize, Pos)> = match &mut self.doc {
            Some(doc) => doc
                .sheets
                .iter_mut()
                .enumerate()
                .flat_map(|(i, s)| {
                    s.recalc();
                    s.shell_cells().into_iter().map(move |p| (i, p))
                })
                .collect(),
            None => return,
        };
        if !self.trusted() && !cells.is_empty() {
            self.problem = Some("This ledger isn't trusted, so its shell cells don't run (File > Trust This Ledger)".into());
        }
        for (i, pos) in cells {
            self.run_shell(ctx, i, pos);
        }
    }

    /// Takes the results of commands that have finished.
    fn poll_shell(&mut self) {
        let Some(doc) = &mut self.doc else {
            while self.finished_rx.try_recv().is_ok() {}
            return;
        };
        for f in self.finished_rx.try_iter() {
            let Some(sheet) = doc.sheets.iter_mut().find(|s| s.id == f.sheet) else { continue };
            sheet.set_pending(f.pos, false);
            // Edited while it ran: the result is for a formula that's gone.
            if sheet.input(f.pos) != f.run.formula {
                continue;
            }
            doc.writer.send(Write::Shell { sheet: f.sheet, pos: f.pos, run: f.run.clone() });
            sheet.set_run(f.pos, f.run);
        }
    }

    // --- Sheets --------------------------------------------------------

    /// Keeps what's being typed in the shown sheet.
    fn finish_edit(&mut self) {
        let Some(doc) = &mut self.doc else { return };
        let Some(sheet) = doc.sheets.get_mut(self.active) else { return };
        if let Some(view) = self.views.get_mut(&sheet.id) {
            let changes = grid::finish(sheet, view);
            send(doc, self.active, changes);
        }
    }

    fn show_sheet(&mut self, index: usize) {
        self.finish_edit();
        if let Some(doc) = &self.doc {
            self.active = index.min(doc.sheets.len() - 1);
        }
    }

    fn new_sheet(&mut self) {
        self.finish_edit();
        let Some(doc) = &mut self.doc else { return };
        let id = doc.sheets.iter().map(|s| s.id).max().unwrap_or(0) + 1;
        let mut n = doc.sheets.len() + 1;
        let name = loop {
            let name = format!("Sheet {n}");
            if !doc.sheets.iter().any(|s| s.name == name) {
                break name;
            }
            n += 1;
        };
        doc.writer.send(Write::AddSheet { id, name: name.clone() });
        doc.sheets.push(Sheet::new(id, name));
        self.active = doc.sheets.len() - 1;
    }

    fn rename_sheet(&mut self, index: usize, name: &str) {
        let name = name.trim();
        let Some(doc) = &mut self.doc else { return };
        if name.is_empty() || doc.sheets[index].name == name {
            return;
        }
        doc.sheets[index].name = name.to_string();
        doc.writer.send(Write::RenameSheet { id: doc.sheets[index].id, name: name.to_string() });
    }

    fn delete_sheet(&mut self, index: usize) {
        self.finish_edit();
        let Some(doc) = &mut self.doc else { return };
        if doc.sheets.len() < 2 {
            self.problem = Some("A ledger needs at least one sheet".into());
            return;
        }
        let sheet = doc.sheets.remove(index);
        self.views.remove(&sheet.id);
        doc.writer.send(Write::DeleteSheet { id: sheet.id });
        if self.active >= index && self.active > 0 {
            self.active -= 1;
        }
    }

    /// A close from the tab or menu: asks first if the sheet has anything in it.
    fn ask_delete(&mut self, index: usize) {
        let Some(doc) = &self.doc else { return };
        if doc.sheets.len() < 2 {
            self.problem = Some("A ledger needs at least one sheet".into());
        } else if doc.sheets[index].is_empty() {
            self.delete_sheet(index);
        } else {
            self.dialog = Some(Dialog::Delete { index });
        }
    }

    fn move_sheet(&mut self, from: usize, to: usize) {
        let Some(doc) = &mut self.doc else { return };
        if from >= doc.sheets.len() || to >= doc.sheets.len() || from == to {
            return;
        }
        let shown = doc.sheets[self.active].id;
        let sheet = doc.sheets.remove(from);
        doc.sheets.insert(to, sheet);
        self.active = doc.sheets.iter().position(|s| s.id == shown).unwrap_or(0);
        doc.writer.send(Write::Order(doc.sheets.iter().map(|s| s.id).collect()));
    }

    // --- Drawing -------------------------------------------------------

    /// Left side of the title bar: word menus, never a hamburger (Patina's ADR-0006).
    fn menus(&self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let has_doc = self.doc.is_some();
        ui.menu_button("File", |ui| {
            item(ui, actions, "New…", "Ctrl+N", true, Action::New);
            item(ui, actions, "Open…", "Ctrl+O", true, Action::Open);
            ui.add_enabled_ui(!self.config.recent.is_empty(), |ui| {
                ui.menu_button("Open Recent", |ui| {
                    for path in &self.config.recent {
                        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        if ui.button(name).on_hover_text(path.display().to_string()).clicked() {
                            actions.push(Action::OpenPath(path.clone()));
                            ui.close();
                        }
                    }
                });
            });
            ui.separator();
            let mut trusted = self.trusted();
            let trust = ui.add_enabled(has_doc, egui::Checkbox::new(&mut trusted, "Trust This Ledger"));
            if trust.on_hover_text("Let this ledger's shell formulas (=SH(...)) run commands on this computer").changed() {
                actions.push(Action::Trust(trusted));
            }
            ui.separator();
            item(ui, actions, "Close", "", has_doc, Action::Close);
            item(ui, actions, "Quit", "Ctrl+Q", true, Action::Quit);
        });
        ui.menu_button("Edit", |ui| {
            item(ui, actions, "Cut", "Ctrl+X", has_doc, Action::Cut);
            item(ui, actions, "Copy", "Ctrl+C", has_doc, Action::Copy);
            item(ui, actions, "Paste", "Ctrl+V", has_doc, Action::Paste);
            item(ui, actions, "Clear", "Del", has_doc, Action::Clear);
        });
        ui.menu_button("Sheet", |ui| {
            let n = self.doc.as_ref().map_or(0, |d| d.sheets.len());
            item(ui, actions, "New Sheet", "Ctrl+T", has_doc, Action::NewSheet);
            item(ui, actions, "Run Shell Cells", "Ctrl+R", has_doc, Action::RunShell);
            ui.separator();
            item(ui, actions, "Rename Sheet…", "", has_doc, Action::Rename(self.active));
            item(ui, actions, "Delete Sheet", "Ctrl+W", n > 1, Action::Delete(self.active));
            ui.separator();
            item(ui, actions, "Move Left", "", self.active > 0, Action::MoveSheet(self.active, self.active.wrapping_sub(1)));
            item(ui, actions, "Move Right", "", self.active + 1 < n, Action::MoveSheet(self.active, self.active + 1));
        });
        ui.menu_button("View", |ui| {
            let mut system = !self.custom_frame;
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

    fn act(&mut self, ctx: &egui::Context, action: Action) {
        match action {
            Action::New => self.pick(ctx, Pick::New),
            Action::Open => self.pick(ctx, Pick::Open),
            Action::OpenPath(path) => self.open_path(&path),
            Action::Close => self.close_document(),
            Action::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Action::Trust(on) => {
                if let Some(doc) = &self.doc {
                    self.config.set_trusted(&doc.path, on);
                }
                if on {
                    self.run_all_shell(ctx);
                }
            }
            Action::RunShell => self.run_all_shell(ctx),
            Action::NewSheet => self.new_sheet(),
            Action::Rename(index) => {
                if let Some(doc) = &self.doc {
                    self.dialog = Some(Dialog::Rename { index, text: doc.sheets[index].name.clone(), fresh: true });
                }
            }
            Action::Delete(index) => self.ask_delete(index),
            Action::MoveSheet(from, to) => self.move_sheet(from, to),
            Action::Cut => ctx.send_viewport_cmd(egui::ViewportCommand::RequestCut),
            Action::Copy => ctx.send_viewport_cmd(egui::ViewportCommand::RequestCopy),
            Action::Paste => ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste),
            Action::Clear => {
                if let Some(doc) = &mut self.doc {
                    let sheet = &mut doc.sheets[self.active];
                    let view = self.views.entry(sheet.id).or_default();
                    let (a, b) = view.selection();
                    let mut changes = Vec::new();
                    for r in a.0..=b.0 {
                        for c in a.1..=b.1 {
                            if sheet.set((r, c), "") {
                                changes.push(((r, c), String::new()));
                            }
                        }
                    }
                    send(doc, self.active, changes);
                }
            }
            Action::SetSkin(name) => {
                self.forced_skin = None;
                self.config.skin = Some(name);
                self.config.save();
                self.apply_skin(ctx);
            }
            Action::SystemTitleBar(on) => {
                self.custom_frame = !on;
                self.config.system_title_bar = on;
                self.config.save();
                ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(on));
            }
        }
    }

    /// Ctrl+N, Ctrl+O, Ctrl+Q, and the tab keys.
    fn shortcuts(&mut self, ctx: &egui::Context) -> Option<TabAction> {
        let pressed = |ctx: &egui::Context, key| ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, key));
        if pressed(ctx, Key::Q) {
            self.act(ctx, Action::Quit);
        }
        if pressed(ctx, Key::N) {
            self.act(ctx, Action::New);
        }
        if pressed(ctx, Key::O) {
            self.act(ctx, Action::Open);
        }
        if pressed(ctx, Key::R) {
            self.act(ctx, Action::RunShell);
        }
        let doc = self.doc.as_ref()?;
        tabs::keys(ctx, doc.sheets.len(), self.active)
    }

    fn apply_tab_action(&mut self, action: TabAction) {
        match action {
            TabAction::New => self.new_sheet(),
            TabAction::Show(i) => self.show_sheet(i),
            TabAction::Close(i) => self.ask_delete(i),
        }
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.dialog.take() else { return };
        let mut keep = true;
        match dialog {
            Dialog::Rename { index, mut text, mut fresh } => {
                let mut done = false;
                egui::Window::new("Rename Sheet")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                    .show(ctx, |ui| {
                        let edit = ui.add(egui::TextEdit::singleline(&mut text).desired_width(240.0));
                        if fresh {
                            edit.request_focus();
                            fresh = false;
                        }
                        ui.horizontal(|ui| {
                            let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                            if ui.button("Rename").clicked() || enter {
                                done = true;
                            }
                            if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
                                keep = false;
                            }
                        });
                    });
                if done {
                    self.rename_sheet(index, &text);
                    keep = false;
                }
                if keep {
                    self.dialog = Some(Dialog::Rename { index, text, fresh });
                }
            }
            Dialog::Delete { index } => {
                let name = self.doc.as_ref().map(|d| d.sheets[index].name.clone()).unwrap_or_default();
                let mut delete = false;
                egui::Window::new("Delete Sheet")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                    .show(ctx, |ui| {
                        ui.label(format!("Delete “{name}” and everything in it? This can't be undone."));
                        ui.horizontal(|ui| {
                            if ui.button("Delete").clicked() {
                                delete = true;
                            }
                            if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
                                keep = false;
                            }
                        });
                    });
                if delete {
                    self.delete_sheet(index);
                } else if keep {
                    self.dialog = Some(Dialog::Delete { index });
                }
            }
        }
    }

    fn status_bar(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if let Some(problem) = &self.problem {
                ui.colored_label(ui.visuals().error_fg_color, problem);
            } else if let Some(doc) = &self.doc {
                let sheet = &doc.sheets[self.active];
                let here = self.views.get(&sheet.id).map(|v| v.cursor).and_then(|p| sheet.run(p));
                let failure = here.and_then(|r| match &r.status {
                    Status::Ok => None,
                    Status::Exit(n) => Some(format!("exit {n}: {}", r.stderr.lines().next().unwrap_or(""))),
                    Status::Timeout => Some("timed out".to_string()),
                    Status::Failed(m) => Some(format!("couldn't start: {m}")),
                });
                if let Some(failure) = failure {
                    ui.colored_label(ui.visuals().error_fg_color, failure);
                } else if !self.trusted() && sheet_has_shell(&doc.sheets) {
                    ui.colored_label(
                        ui.visuals().warn_fg_color,
                        "Shell cells show their last results and won't run: File > Trust This Ledger",
                    );
                } else {
                    ui.weak(doc.path.display().to_string());
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let Some(doc) = &self.doc else { return };
                let sheet = &doc.sheets[self.active];
                let Some(view) = self.views.get(&sheet.id) else { return };
                let (a, b) = view.selection();
                ui.label(grid::range_name(a, b));
                if a != b {
                    let nums = sheet.numbers_in(a, b);
                    if !nums.is_empty() {
                        let sum: f64 = nums.iter().sum();
                        ui.separator();
                        ui.label(format!(
                            "Sum {}   Average {}   Count {}",
                            formula::format_number(sum),
                            formula::format_number(sum / nums.len() as f64),
                            nums.len()
                        ));
                    }
                }
            });
        });
    }

    fn take_screenshot(&mut self, ctx: &egui::Context) {
        let Some(shot) = &mut self.screenshot else { return };
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
                .and_then(|b| b.save(&shot.path).map_err(|e| e.to_string()));
            match saved {
                Ok(()) => eprintln!("saved {}", shot.path.display()),
                Err(e) => eprintln!("screenshot failed: {e}"),
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else if !shot.requested && ctx.input(|i| i.time) > 1.0 {
            shot.requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
    }
}

fn sheet_has_shell(sheets: &[Sheet]) -> bool {
    sheets.iter().any(|s| !s.shell_cells().is_empty())
}

/// A menu entry that queues `action` when clicked.
fn item(ui: &mut egui::Ui, actions: &mut Vec<Action>, label: &str, shortcut: &str, enabled: bool, action: Action) {
    if ui.add_enabled(enabled, egui::Button::new(label).shortcut_text(shortcut)).clicked() {
        actions.push(action);
        ui.close();
    }
}

/// Sends a sheet's changed cells to the file.
fn send(doc: &Document, sheet: usize, changes: grid::Changes) {
    if changes.is_empty() {
        return;
    }
    let id = doc.sheets[sheet].id;
    for (pos, input) in changes {
        doc.writer.send(Write::Cell { sheet: id, pos, input });
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        if self.custom_frame {
            patina::frame::shadow::request(frame);
        }
        let ctx = ui.ctx().clone();
        self.poll_picker();
        self.poll_shell();
        if let Some(problem) = self.doc.as_ref().and_then(|d| d.writer.error()) {
            self.problem = Some(problem);
        }
        // Every frame: Default's colours follow the desktop's light/dark setting.
        let style = self.skin().frame_style(ui.visuals());
        let radius = if self.custom_frame { frame::corner_radius(&ctx, style.radius) } else { 0 };

        let mut tab_action = if self.dialog.is_none() { self.shortcuts(&ctx) } else { None };
        let mut actions = Vec::new();
        let titles: Vec<String> = self.doc.as_ref().map(|d| d.sheets.iter().map(|s| s.name.clone()).collect()).unwrap_or_default();
        let mut moved = None;
        let mut rename = None;
        egui::Panel::top("header").frame(frame::header_frame(ui.style(), &style, radius)).show(ui, |ui| {
            let span = frame::header_span(ui);
            TitleBar::new(if titles.is_empty() { "Ledger" } else { "" }, &style)
                .custom(self.custom_frame)
                .menu_bar(true)
                .span(span)
                .show(
                    ui,
                    |ui| {
                        self.menus(ui, &mut actions);
                        if !titles.is_empty() {
                            let mut on_move = |from: usize, to: usize| moved = Some((from, to));
                            let mut tab_menu = |ui: &mut egui::Ui, i: usize| {
                                if ui.button("Rename…").clicked() {
                                    rename = Some(i);
                                    ui.close();
                                }
                            };
                            let strip = TabStrip::new(&titles, self.active, &style)
                                .span(span)
                                .noun("Sheet")
                                .new_hint("New Sheet (Ctrl+T)")
                                .movable(&mut on_move)
                                .tab_menu(&mut tab_menu);
                            tab_action = tab_action.or(strip.show(ui));
                        }
                    },
                    |_ui| {
                        // Right side: the app's output. A ledger's output is its sheets,
                        // which are the tabs, so there's nothing here yet.
                    },
                );
        });
        if let Some((from, to)) = moved {
            self.move_sheet(from, to);
        }
        if let Some(i) = rename {
            self.act(&ctx, Action::Rename(i));
        }
        if let Some(a) = tab_action {
            self.apply_tab_action(a);
        }
        for action in actions {
            self.act(&ctx, action);
        }

        // The status bar holds the bottom corners.
        let corners = egui::CornerRadius { sw: radius, se: radius, ..Default::default() };
        egui::Panel::bottom("status").frame(egui::Frame::side_top_panel(ui.style()).corner_radius(corners)).show(ui, |ui| {
            self.status_bar(ui);
        });

        let dialog_open = self.dialog.is_some();
        let mut start = None;
        let mut new_shell: Vec<(usize, Pos)> = Vec::new();
        egui::CentralPanel::default().frame(egui::Frame::central_panel(ui.style()).inner_margin(0)).show(ui, |ui| {
            let Some(doc) = &mut self.doc else {
                ui.vertical_centered(|ui| {
                    ui.add_space(ui.available_height() * 0.3);
                    ui.heading("Ledger");
                    ui.weak("Bookkeeping in tabbed spreadsheets, stored as SQLite databases");
                    ui.add_space(12.0);
                    if ui.button("New ledger…").clicked() {
                        start = Some(Pick::New);
                    }
                    if ui.button("Open…").clicked() {
                        start = Some(Pick::Open);
                    }
                });
                return;
            };
            let index = self.active.min(doc.sheets.len() - 1);
            let sheet = &mut doc.sheets[index];
            let view = self.views.entry(sheet.id).or_default();

            // The formula bar: the cell, and what it holds.
            egui::Frame::new().inner_margin(egui::Margin::symmetric(8, 4)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add_sized([56.0, 18.0], egui::Label::new(egui::RichText::new(cell_name(view.cursor)).strong()));
                    ui.separator();
                    let input = sheet.input(view.cursor);
                    ui.add(egui::Label::new(egui::RichText::new(input).monospace()).truncate());
                });
            });
            let changes = grid::show(ui, sheet, view, !dialog_open);
            // A shell formula that was just entered runs, in a trusted ledger.
            new_shell.extend(changes.iter().filter(|(_, input)| is_shell(input)).map(|(pos, _)| (index, *pos)));
            sheet.recalc();
            send(doc, index, changes);
        });

        for (index, pos) in new_shell {
            self.run_shell(&ctx, index, pos);
        }
        if let Some(kind) = start {
            self.pick(&ctx, kind);
        }
        self.dialogs(&ctx);

        if self.custom_frame {
            frame::edges(&ctx, radius, style.border);
        }
        let title = match &self.doc {
            Some(doc) => format!("{} — Ledger", doc.path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default()),
            None => "Ledger".into(),
        };
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
        self.take_screenshot(&ctx);
    }

    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        frame::clear_color(visuals, self.custom_frame)
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        frame::release_grab(ctx, raw_input);
    }
}

/// Runs a file dialog in a thread, so the window keeps drawing.
fn spawn_dialog(ctx: egui::Context, kind: Pick, tx: mpsc::Sender<Option<PathBuf>>) {
    std::thread::spawn(move || {
        let dialog = rfd::FileDialog::new().add_filter("Ledger", &[EXTENSION, "sqlite", "db"]);
        let path = match kind {
            Pick::Open => dialog.set_title("Open a ledger").pick_file(),
            Pick::New => dialog.set_title("New ledger").set_file_name(format!("Books.{EXTENSION}")).save_file(),
        };
        let _ = tx.send(path);
        ctx.request_repaint();
    });
}
