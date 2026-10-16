//! The window: Patina's frame with File, Go and Skin menus, the project
//! tree on the left with a filter, and the file on the right: markdown
//! through Patina's read-only editor, Rust highlighted with line numbers.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use egui::{Align, CornerRadius, Id, Key, Modifiers, RichText, Sense, Ui, vec2};
use patina::frame::{self, FrameStyle, TitleBar};
use patina::markdown::{Editor, embeds};
use patina::skin::{self, Skin};

use crate::config::Config;
use crate::links::{self, Target};
use crate::man;
use crate::tree::{self, Kind, Node};

/// Reader's skins folder is `~/.config/reader/skins`.
const APP: &str = "reader";
/// The widest a markdown document's column grows, for comfortable lines.
const COLUMN: f32 = 820.0;
/// Each level of the tree, indented.
const INDENT: f32 = 14.0;

/// The file on screen.
enum Shown {
    Markdown(Box<Editor>),
    Man(man::Page),
    Rust(String),
    Error(String),
}

pub struct Reader {
    config: Config,
    skins: Vec<Skin>,
    skin: usize,
    skin_errors: Vec<String>,
    root: Option<PathBuf>,
    nodes: Vec<Node>,
    filter: String,
    focus_filter: bool,
    /// The name typed in Go's Man Page field.
    man_query: String,
    /// Folders open in the tree.
    expanded: HashSet<PathBuf>,
    path: Option<PathBuf>,
    shown: Option<Shown>,
    back: Vec<PathBuf>,
    forward: Vec<PathBuf>,
    /// Scroll the tree to the file on screen, after following a link.
    reveal: bool,
    /// Something to say under the file, until the next one opens.
    notice: Option<String>,
    title: String,
    screenshot: Option<(PathBuf, bool)>,
    /// Single-file mode: no tree, and the last project read is left alone.
    single: bool,
    /// System mode: the tree is every man page on the system, by section.
    man: bool,
}

impl Reader {
    /// Opens `root` at `file`, or the last project read; `single` shows
    /// the file alone, without the tree; `man` shows the system's man pages
    /// as the tree, at `file` or man(1).
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        root: Option<PathBuf>,
        file: Option<PathBuf>,
        single: bool,
        man: bool,
    ) -> Self {
        patina::appearance::watch(&cc.egui_ctx);
        patina::markdown::install(&cc.egui_ctx);
        let config = Config::load();
        let (skins, skin_errors) = load_skins();
        let skin = skins.iter().position(|s| s.name == config.skin).unwrap_or(0);
        let mut reader = Reader {
            skins,
            skin,
            skin_errors,
            root: None,
            nodes: Vec::new(),
            filter: String::new(),
            focus_filter: false,
            man_query: String::new(),
            expanded: HashSet::new(),
            path: None,
            shown: None,
            back: Vec::new(),
            forward: Vec::new(),
            reveal: false,
            notice: None,
            title: String::new(),
            screenshot: std::env::var_os("READER_SCREENSHOT").map(|p| (PathBuf::from(p), false)),
            single,
            man,
            config,
        };
        reader.apply_skin(&cc.egui_ctx);
        if reader.config.system_title_bar {
            cc.egui_ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(true));
        }
        if man {
            reader.nodes = tree::man_pages();
            if let Some(page) = file.or_else(|| man::find("man")) {
                reader.show(page);
            }
            return reader;
        }
        // What was asked for, else where the last reading left off.
        let (root, file) = match root {
            Some(root) => (Some(root), file),
            None => {
                let last = reader.config.last_root.clone().filter(|r| r.is_dir());
                let file = reader.config.last_file.clone().filter(|f| last.as_ref().is_some_and(|r| f.starts_with(r)));
                (last, file)
            }
        };
        if let Some(root) = root {
            reader.open_project(root, file.filter(|f| f.is_file()));
        }
        reader
    }

    fn apply_skin(&self, ctx: &egui::Context) {
        let skin = &self.skins[self.skin];
        skin.apply(ctx);
        if skin.follows_desktop() {
            ctx.set_theme(self.config.theme());
        }
    }

    /// Reads `root`'s tree and shows `file`, or the project's README.
    fn open_project(&mut self, root: PathBuf, file: Option<PathBuf>) {
        self.man = false;
        self.nodes = tree::scan(&root);
        self.expanded.clear();
        self.back.clear();
        self.forward.clear();
        self.filter.clear();
        self.root = Some(root.clone());
        if self.remembers() {
            self.config.last_root = Some(root);
        }
        self.path = None;
        self.shown = None;
        if let Some(file) = file.or_else(|| tree::first_file(&self.nodes)) {
            self.show(file);
        } else if self.remembers() {
            self.config.save();
        }
    }

    /// Man mode: every man page on the system as the tree, at man(1).
    fn open_manual(&mut self) {
        self.man = true;
        self.root = None;
        self.nodes = tree::man_pages();
        self.expanded.clear();
        self.back.clear();
        self.forward.clear();
        self.filter.clear();
        self.path = None;
        self.shown = None;
        if let Some(page) = man::find("man") {
            self.show(page);
        }
    }

    /// Whether what's read is remembered for next time: not in single-file
    /// or man mode, which help and lookups use.
    fn remembers(&self) -> bool {
        !self.single && !self.man
    }

    /// Shows `path`, read afresh from disk.
    fn show(&mut self, path: PathBuf) {
        self.notice = None;
        let shown = match Kind::of(&path) {
            // The formatter reads the page itself, gzipped or not.
            Some(Kind::Man) => match man::render(&path) {
                Ok(page) => Shown::Man(page),
                Err(e) => Shown::Error(format!("Can't format {}: {e}", path.display())),
            },
            kind => self.read(&path, kind),
        };
        // Its folders open in the tree, so it can be seen there: a man
        // page's section, or the folders above a project's file.
        if self.man
            && let Some(section) = man::section_folder(&path)
        {
            self.expanded.insert(section);
        }
        if let Some(root) = &self.root {
            let mut at = path.parent();
            while let Some(dir) = at.filter(|d| d.starts_with(root) && *d != root.as_path()) {
                self.expanded.insert(dir.to_path_buf());
                at = dir.parent();
            }
        }
        self.shown = Some(shown);
        self.path = Some(path.clone());
        self.reveal = true;
        // A single file, as help opens, isn't the project being read.
        if self.remembers() {
            self.config.last_file = Some(path);
            self.config.save();
        }
    }

    /// A markdown or source file, read from disk.
    fn read(&self, path: &Path, kind: Option<Kind>) -> Shown {
        match (std::fs::read_to_string(path), kind) {
            (Err(e), _) => Shown::Error(format!("Can't read {}: {e}", path.display())),
            (Ok(text), Some(Kind::Markdown)) => {
                let mut editor = Editor::new(Id::new(("document", path)));
                editor.set_read_only(true);
                editor.set_dir(path.parent().map(Path::to_path_buf));
                editor.load(&text);
                Shown::Markdown(Box::new(editor))
            }
            (Ok(text), _) => Shown::Rust(text),
        }
    }

    /// Goes to `path`, keeping where it was for Back.
    fn go(&mut self, path: PathBuf) {
        if self.path.as_ref() == Some(&path) {
            return;
        }
        if let Some(here) = self.path.take() {
            self.back.push(here);
        }
        self.forward.clear();
        self.show(path);
    }

    fn back(&mut self) {
        if let Some(previous) = self.back.pop() {
            if let Some(here) = self.path.take() {
                self.forward.push(here);
            }
            self.show(previous);
        }
    }

    fn forward(&mut self) {
        if let Some(next) = self.forward.pop() {
            if let Some(here) = self.path.take() {
                self.back.push(here);
            }
            self.show(next);
        }
    }

    fn reload(&mut self) {
        if self.man {
            self.nodes = tree::man_pages();
        } else if let Some(root) = self.root.clone() {
            self.nodes = tree::scan(&root);
        }
        if let Some(path) = self.path.clone() {
            self.show(path);
        }
    }

    /// Follows a link clicked in the document.
    fn follow(&mut self, link: &str, ctx: &egui::Context) {
        let Some(from) = self.path.clone() else { return };
        match links::resolve(link, &from) {
            Target::Web(url) => ctx.open_url(egui::OpenUrl::new_tab(url)),
            Target::Read(path) if path.is_file() => self.go(path),
            Target::Read(path) => self.notice = Some(format!("There's no {}", self.relative(&path))),
            Target::Open(path) => {
                if let Err(e) = patina::open::path(&path) {
                    self.notice = Some(format!("Can't open {}: {e}", self.relative(&path)));
                }
            }
            Target::Stay => {}
        }
    }

    fn pick_folder(&mut self) {
        let mut dialog = rfd::FileDialog::new().set_title("Open a Project");
        if let Some(root) = &self.root {
            dialog = dialog.set_directory(root);
        }
        if let Some(root) = dialog.pick_folder() {
            self.open_project(root, None);
        }
    }

    /// `path` from the project's folder, for showing; a man page by its
    /// name in man mode.
    fn relative(&self, path: &Path) -> String {
        if self.man && Kind::of(path) == Some(Kind::Man) {
            return man::label(path);
        }
        match &self.root {
            Some(root) => path.strip_prefix(root).unwrap_or(path).display().to_string(),
            None => path.display().to_string(),
        }
    }

    fn keys(&mut self, ctx: &egui::Context) {
        let (back, forward, open, reload, find, quit) = ctx.input_mut(|i| {
            let mouse_back = i.pointer.button_pressed(egui::PointerButton::Extra1);
            let mouse_forward = i.pointer.button_pressed(egui::PointerButton::Extra2);
            (
                i.consume_key(Modifiers::ALT, Key::ArrowLeft) || mouse_back,
                i.consume_key(Modifiers::ALT, Key::ArrowRight) || mouse_forward,
                i.consume_key(Modifiers::COMMAND, Key::O),
                i.consume_key(Modifiers::COMMAND, Key::R) || i.consume_key(Modifiers::NONE, Key::F5),
                i.consume_key(Modifiers::COMMAND, Key::F),
                i.consume_key(Modifiers::COMMAND, Key::Q) || i.consume_key(Modifiers::COMMAND, Key::W),
            )
        });
        if back {
            self.back();
        }
        if forward {
            self.forward();
        }
        if open {
            self.pick_folder();
        }
        if reload {
            self.reload();
        }
        if find {
            self.focus_filter = true;
        }
        if quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    // ---- Drawing ----

    fn header(&mut self, ui: &mut Ui, style: &FrameStyle) {
        let title = self.title.clone();
        TitleBar::new(&title, style).custom(!self.config.system_title_bar).menu_bar(true).show(
            ui,
            |ui| {
                ui.menu_button("File", |ui| self.file_menu(ui));
                ui.menu_button("Go", |ui| self.go_menu(ui));
                ui.menu_button("Skin", |ui| self.skin_menu(ui));
            },
            |_| {},
        );
    }

    fn file_menu(&mut self, ui: &mut Ui) {
        if ui.add(egui::Button::new("Open Project…").shortcut_text(weak(ui, "Ctrl+O"))).clicked() {
            self.pick_folder();
        }
        if ui.add_enabled(self.path.is_some(), egui::Button::new("Reload").shortcut_text(weak(ui, "Ctrl+R"))).clicked()
        {
            self.reload();
        }
        let mut tree = !self.single;
        if ui.add(egui::Checkbox::new(&mut tree, "Project Tree")).changed() {
            self.single = !tree;
        }
        let folder = self.path.as_ref().and_then(|p| p.parent()).map(Path::to_path_buf);
        if ui.add_enabled(folder.is_some(), egui::Button::new("Show in Files")).clicked()
            && let Some(folder) = folder
        {
            let _ = patina::open::path(&folder);
        }
        if ui.add_enabled(self.path.is_some(), egui::Button::new("Copy Path")).clicked()
            && let Some(path) = &self.path
        {
            ui.ctx().copy_text(path.display().to_string());
        }
        ui.separator();
        let mut patina_frame = !self.config.system_title_bar;
        if ui.checkbox(&mut patina_frame, "Patina Frame").changed() {
            self.config.system_title_bar = !patina_frame;
            self.config.save();
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Decorations(!patina_frame));
        }
        ui.separator();
        if ui.add(egui::Button::new("Quit").shortcut_text(weak(ui, "Ctrl+Q"))).clicked() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn go_menu(&mut self, ui: &mut Ui) {
        if ui
            .add_enabled(!self.back.is_empty(), egui::Button::new("Back").shortcut_text(weak(ui, "Alt+Left")))
            .clicked()
        {
            self.back();
        }
        if ui
            .add_enabled(!self.forward.is_empty(), egui::Button::new("Forward").shortcut_text(weak(ui, "Alt+Right")))
            .clicked()
        {
            self.forward();
        }
        ui.separator();
        let readme = tree::first_file(&self.nodes).filter(|_| !self.man);
        if ui.add_enabled(readme.is_some(), egui::Button::new("Project README")).clicked()
            && let Some(readme) = readme
        {
            self.go(readme);
        }
        if ui.add_enabled(!self.man, egui::Button::new("System Manual")).clicked() {
            self.open_manual();
        }
        if ui.add(egui::Button::new("Find File").shortcut_text(weak(ui, "Ctrl+F"))).clicked() {
            self.focus_filter = true;
        }
        ui.separator();
        ui.label("Man Page:");
        let field =
            ui.add(egui::TextEdit::singleline(&mut self.man_query).hint_text("ls, printf(3)").desired_width(160.0));
        if field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
            let query = std::mem::take(&mut self.man_query);
            match man::find(&query) {
                Some(page) => self.go(page),
                None => self.notice = Some(format!("No man page for {query}")),
            }
            ui.close();
        }
    }

    fn skin_menu(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let mut changed = false;
        for (i, skin) in self.skins.iter().enumerate() {
            changed |= ui.radio_value(&mut self.skin, i, &skin.name).clicked();
        }
        ui.separator();
        ui.add_enabled_ui(self.skins[self.skin].follows_desktop(), |ui| {
            for (value, label) in [("system", "Follow Desktop"), ("light", "Light"), ("dark", "Dark")] {
                if ui.radio(self.config.light_dark == value, label).clicked() {
                    self.config.light_dark = value.into();
                    changed = true;
                }
            }
        });
        ui.separator();
        if ui.button("Reload Skins").clicked() {
            let name = self.skins[self.skin].name.clone();
            (self.skins, self.skin_errors) = load_skins();
            self.skin = self.skins.iter().position(|s| s.name == name).unwrap_or(0);
            changed = true;
        }
        if let Some(dir) = skin::app_dir(APP)
            && ui.button("Open Skins Folder").on_hover_text(dir.display().to_string()).clicked()
        {
            let _ = std::fs::create_dir_all(&dir);
            let _ = patina::open::path(&dir);
        }
        for error in &self.skin_errors {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        if changed {
            self.config.skin = self.skins[self.skin].name.clone();
            self.config.save();
            self.apply_skin(&ctx);
        }
    }

    fn sidebar(&mut self, ui: &mut Ui) {
        ui.add_space(8.0);
        if self.man {
            ui.label(RichText::new("Manual").strong()).on_hover_text("Every man page on this system");
        } else if let Some(root) = &self.root {
            let name =
                root.file_name().map_or_else(|| root.display().to_string(), |n| n.to_string_lossy().into_owned());
            ui.label(RichText::new(name).strong()).on_hover_text(root.display().to_string());
        }
        let filter = ui.add(
            egui::TextEdit::singleline(&mut self.filter)
                .hint_text(if self.man { "Find a page" } else { "Find a file" })
                .desired_width(f32::INFINITY),
        );
        if std::mem::take(&mut self.focus_filter) {
            filter.request_focus();
        }
        if filter.has_focus() && ui.input(|i| i.key_pressed(Key::Escape)) {
            self.filter.clear();
        }
        let enter = filter.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        ui.add_space(4.0);
        let nodes = tree::filtered(&self.nodes, &self.filter);
        let filtering = !self.filter.trim().is_empty();
        if enter
            && filtering
            && let Some(first) = first_match(&nodes)
        {
            self.go(first);
        }
        let mut picked = None;
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            self.tree(ui, &nodes, 0, filtering, &mut picked);
        });
        self.reveal = false;
        if let Some(path) = picked {
            self.go(path);
        }
    }

    fn tree(&mut self, ui: &mut Ui, nodes: &[Node], depth: usize, filtering: bool, picked: &mut Option<PathBuf>) {
        for node in nodes {
            match node {
                Node::Folder { name, path, children } => {
                    let open = filtering || self.expanded.contains(path);
                    let resp = row(ui, depth, Mark::Folder(open), name, false);
                    if resp.clicked() && !filtering && !self.expanded.remove(path) {
                        self.expanded.insert(path.clone());
                    }
                    if open {
                        self.tree(ui, children, depth + 1, filtering, picked);
                    }
                }
                Node::File { name, path, kind } => {
                    let current = self.path.as_ref() == Some(path);
                    let mark = match kind {
                        Kind::Markdown => "md",
                        Kind::Man => "man",
                        Kind::Rust => "rs",
                    };
                    let resp = row(ui, depth, Mark::Kind(mark), name, current);
                    if current && self.reveal {
                        resp.scroll_to_me(Some(Align::Center));
                    }
                    if resp.clicked() {
                        *picked = Some(path.clone());
                    }
                }
            }
        }
    }

    fn content(&mut self, ui: &mut Ui) {
        let Some(path) = self.path.clone() else {
            self.welcome(ui);
            return;
        };
        ui.horizontal(|ui| {
            ui.label(RichText::new(self.relative(&path)).weak().small());
            if let Some(notice) = &self.notice {
                ui.colored_label(ui.visuals().error_fg_color, RichText::new(notice).small());
            }
        });
        ui.add_space(4.0);
        let mut link = None;
        match &mut self.shown {
            Some(Shown::Markdown(editor)) => {
                egui::ScrollArea::vertical().id_salt(("markdown", &path)).auto_shrink(false).show(ui, |ui| {
                    // A readable column, centred in a wide window.
                    let width = ui.available_width().min(COLUMN);
                    let side = (ui.available_width() - width) / 2.0;
                    ui.horizontal_top(|ui| {
                        ui.add_space(side);
                        ui.vertical(|ui| {
                            ui.set_width(width);
                            editor.show(ui);
                        });
                    });
                });
                link = editor.take_link();
            }
            Some(Shown::Man(page)) => {
                egui::ScrollArea::both().id_salt(("man", &path)).auto_shrink(false).show(ui, |ui| {
                    man::show(ui, page, Id::new(("man", &path)));
                });
            }
            Some(Shown::Rust(text)) => {
                // Source sits on the skin's code background, as code blocks do.
                egui::Frame::new()
                    .fill(ui.visuals().code_bg_color)
                    .corner_radius(6)
                    .inner_margin(egui::Margin::symmetric(14, 10))
                    .show(ui, |ui| {
                        ui.set_min_size(ui.available_size());
                        egui::ScrollArea::both().id_salt(("rust", &path)).auto_shrink(false).show(ui, |ui| {
                            embeds::code_file(ui, text, "rs", Id::new(("source", &path)));
                        });
                    });
            }
            Some(Shown::Error(message)) => {
                ui.colored_label(ui.visuals().error_fg_color, message.as_str());
            }
            None => {}
        }
        if let Some(link) = link {
            self.follow(&link, ui.ctx());
        }
    }

    fn welcome(&mut self, ui: &mut Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() / 3.0);
            ui.heading("Reader");
            ui.label(RichText::new("Point it at a project to read its documentation and code.").weak());
            ui.add_space(12.0);
            if ui.button("Open Project…").clicked() {
                self.pick_folder();
            }
            if self.root.is_some() {
                ui.add_space(8.0);
                ui.label(RichText::new("Nothing to read here: no markdown or Rust files.").weak());
            }
        });
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let title = match &self.path {
            Some(path) if Kind::of(path) == Some(Kind::Man) => man::label(path),
            Some(path) => path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
            None => "Reader".to_string(),
        };
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    /// In screenshot mode: waits for the page, saves the window to
    /// `READER_SCREENSHOT` and quits.
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
            let mut png = Vec::new();
            let saved = egui_png(&mut png, w as u32, h as u32, image.as_raw())
                .and_then(|()| std::fs::write(&*path, &png).map_err(|e| e.to_string()));
            match saved {
                Ok(()) => eprintln!("saved {}", path.display()),
                Err(e) => eprintln!("reader: screenshot failed: {e}"),
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else if !*requested && ctx.input(|i| i.time) > 2.0 {
            *requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
    }
}

impl eframe::App for Reader {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let custom = !self.config.system_title_bar;
        let style = self.skins[self.skin].frame_style(ui.visuals());
        let radius = if custom { frame::corner_radius(&ctx, style.radius) } else { 0 };
        self.keys(&ctx);
        self.update_title(&ctx);

        let header = frame::header_frame(ui.style(), &style, radius);
        egui::Panel::top("header").frame(header).show(ui, |ui| self.header(ui, &style));
        if !self.single {
            egui::Panel::left("tree")
                .frame(
                    egui::Frame::side_top_panel(ui.style())
                        .inner_margin(egui::Margin::symmetric(8, 0))
                        .corner_radius(CornerRadius { sw: radius, ..Default::default() }),
                )
                .default_size(240.0)
                .size_range(160.0..=480.0)
                .show(ui, |ui| self.sidebar(ui));
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(ui.style()).inner_margin(egui::Margin::symmetric(24, 12)).corner_radius(
                CornerRadius { se: radius, sw: if self.single { radius } else { 0 }, ..Default::default() },
            ))
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
        patina::ime::drop_repeated_preedits(ctx, raw_input);
    }

    fn persist_egui_memory(&self) -> bool {
        false
    }
}

/// What a row shows before its name.
enum Mark<'a> {
    /// A folder's arrow, pointing down when it's open.
    Folder(bool),
    /// A file's kind, as a short word.
    Kind(&'a str),
}

/// A row of the tree: a small mark (a folder's arrow, a file's kind) and
/// the name, indented by depth, filled when it's the file on screen.
fn row(ui: &mut Ui, depth: usize, mark: Mark, name: &str, current: bool) -> egui::Response {
    let height = 22.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
    let v = ui.visuals();
    if current {
        ui.painter().rect_filled(rect, 4.0, v.selection.bg_fill);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, 4.0, v.widgets.hovered.weak_bg_fill);
    }
    let text = if current { v.selection.stroke.color } else { v.text_color() };
    let x = rect.left() + 6.0 + depth as f32 * INDENT;
    let small = egui::TextStyle::Small.resolve(ui.style());
    let body = egui::TextStyle::Body.resolve(ui.style());
    let mark_colour = if current { text } else { v.weak_text_color() };
    match mark {
        // Drawn, not a character: the UI font has no arrows.
        Mark::Folder(open) => {
            let c = egui::pos2(x + 5.0, rect.center().y);
            let points = if open {
                vec![c + vec2(-4.0, -2.0), c + vec2(4.0, -2.0), c + vec2(0.0, 3.0)]
            } else {
                vec![c + vec2(-2.0, -4.0), c + vec2(3.0, 0.0), c + vec2(-2.0, 4.0)]
            };
            ui.painter().add(egui::Shape::convex_polygon(points, mark_colour, egui::Stroke::NONE));
        }
        Mark::Kind(word) => {
            ui.painter().text(egui::pos2(x, rect.center().y), egui::Align2::LEFT_CENTER, word, small, mark_colour);
        }
    }
    ui.painter_at(rect).text(egui::pos2(x + 26.0, rect.center().y), egui::Align2::LEFT_CENTER, name, body, text);
    resp
}

/// The first file in a filtered tree, for Enter in the filter.
fn first_match(nodes: &[Node]) -> Option<PathBuf> {
    nodes.iter().find_map(|n| match n {
        Node::File { path, .. } => Some(path.clone()),
        Node::Folder { children, .. } => first_match(children),
    })
}

fn weak(ui: &Ui, text: &str) -> RichText {
    RichText::new(text).color(ui.visuals().weak_text_color())
}

fn load_skins() -> (Vec<Skin>, Vec<String>) {
    skin::load_all(skin::BUILT_IN, &skin::app_dir(APP).into_iter().collect::<Vec<_>>())
}

/// RGBA pixels as a PNG, for screenshot mode, without an image crate of
/// our own: egui's loaders bring one in through Patina.
fn egui_png(out: &mut Vec<u8>, w: u32, h: u32, rgba: &[u8]) -> Result<(), String> {
    let image = image::RgbaImage::from_raw(w, h, rgba.to_vec()).ok_or("the screenshot has the wrong size")?;
    image.write_to(&mut std::io::Cursor::new(out), image::ImageFormat::Png).map_err(|e| e.to_string())
}
