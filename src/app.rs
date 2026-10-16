//! The window: Patina's frame with File, Go and Skin menus, tabs (each a
//! project, or the system's manual) and a search field in the title bar,
//! the tab's tree on the left with a filter, and its file on the right:
//! markdown through Patina's read-only editor, man pages formatted by the
//! system, Rust highlighted with line numbers.

use std::collections::HashSet;
use std::ops::Range;
use std::path::{Path, PathBuf};

use egui::{Align, CornerRadius, Id, Key, Modifiers, RichText, Sense, TextWrapMode, Ui, vec2};
use patina::frame::{self, FrameStyle, TitleBar};
use patina::markdown::{Editor, embeds};
use patina::skin::{self, Skin};
use patina::tabs::{self, TabAction, TabStrip};

use crate::config::{Config, SavedTab};
use crate::links::{self, Target};
use crate::man;
use crate::search;
use crate::tree::{self, Kind, Node};
use crate::watch::{Changes, Watch};

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

/// What search found in the file on screen: for man pages and source the
/// places, for markdown only the count (the editor keeps the places).
#[derive(Default)]
struct Found {
    marks: Vec<Range<usize>>,
    /// Which one is current, from 0, and how many there are.
    current: Option<usize>,
    total: usize,
    /// Bring the current one into view on the next frame.
    scroll: bool,
}

/// A tab: a project, or the system's man pages, with its own tree,
/// history and file on screen.
struct Tab {
    root: Option<PathBuf>,
    /// The system's man pages rather than a project.
    man: bool,
    nodes: Vec<Node>,
    /// The project's folders, watched for changes.
    folders: Vec<PathBuf>,
    filter: String,
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
    found: Found,
}

impl Tab {
    fn empty() -> Self {
        Tab {
            root: None,
            man: false,
            nodes: Vec::new(),
            folders: Vec::new(),
            filter: String::new(),
            expanded: HashSet::new(),
            path: None,
            shown: None,
            back: Vec::new(),
            forward: Vec::new(),
            reveal: false,
            notice: None,
            found: Found::default(),
        }
    }

    /// A project, at `file` or its README.
    fn project(root: PathBuf, file: Option<PathBuf>) -> Self {
        let mut tab = Tab::empty();
        tab.root = Some(root);
        tab.rescan();
        if let Some(file) = file.filter(|f| f.is_file()).or_else(|| tree::first_file(&tab.nodes)) {
            tab.show(file);
        }
        tab
    }

    /// The system's man pages, at `page` or man(1).
    fn manual(page: Option<PathBuf>) -> Self {
        let mut tab = Tab::empty();
        tab.man = true;
        tab.nodes = tree::man_pages();
        if let Some(page) = page.filter(|p| p.is_file()).or_else(|| man::find("man")) {
            tab.show(page);
        }
        tab
    }

    fn title(&self) -> String {
        match (&self.root, self.man) {
            (_, true) => "Manual".into(),
            (Some(root), _) => {
                root.file_name().map_or_else(|| root.display().to_string(), |n| n.to_string_lossy().into_owned())
            }
            (None, _) => "New Tab".into(),
        }
    }

    fn saved(&self) -> SavedTab {
        SavedTab { root: self.root.clone(), man: self.man, file: self.path.clone() }
    }

    /// Shows `path`, read afresh from disk.
    fn show(&mut self, path: PathBuf) {
        self.notice = None;
        self.found = Found::default();
        self.shown = Some(match Kind::of(&path) {
            // The formatter reads the page itself, gzipped or not.
            Some(Kind::Man) => match man::render(&path) {
                Ok(page) => Shown::Man(page),
                Err(e) => Shown::Error(format!("Can't format {}: {e}", path.display())),
            },
            kind => read(&path, kind),
        });
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
        self.path = Some(path);
        self.reveal = true;
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
        } else {
            self.rescan();
        }
        if let Some(path) = self.path.clone() {
            self.show(path);
        }
    }

    /// Reads the project's tree and folders again.
    fn rescan(&mut self) {
        if let Some(root) = &self.root {
            self.nodes = tree::scan(root);
            self.folders = tree::folders(root);
        }
    }

    /// The folders to watch: the project's, or a lone file's (watching its
    /// folder catches editors that save by replacing the file). Not the
    /// system's man pages.
    fn watched(&self) -> Vec<&Path> {
        match (&self.root, &self.path) {
            _ if self.man => Vec::new(),
            (Some(_), _) => self.folders.iter().map(PathBuf::as_path).collect(),
            (None, Some(path)) => path.parent().into_iter().collect(),
            (None, None) => Vec::new(),
        }
    }

    /// Takes in what changed on disk: the tree scanned again if its
    /// folders changed, the file on screen read again if it did, keeping
    /// its place, history and notice. Whether the file was read again.
    fn changed(&mut self, changes: &Changes) -> bool {
        if self.man {
            return false;
        }
        if let Some(root) = &self.root
            && (changes.everything || changes.folders.iter().any(|f| f.starts_with(root)))
        {
            self.rescan();
        }
        let Some(path) = self.path.clone() else { return false };
        if !changes.everything && !changes.paths.contains(&path) {
            return false;
        }
        let (notice, reveal, current) = (self.notice.take(), self.reveal, self.found.current);
        self.show(path);
        (self.notice, self.reveal, self.found.current) = (notice, reveal, current);
        true
    }

    /// `path` from the project's folder, for showing; a man page by its
    /// man name.
    fn relative(&self, path: &Path) -> String {
        if Kind::of(path) == Some(Kind::Man) && (self.man || self.root.as_ref().is_none_or(|r| !path.starts_with(r))) {
            return man::label(path);
        }
        match &self.root {
            Some(root) => path.strip_prefix(root).unwrap_or(path).display().to_string(),
            None => path.display().to_string(),
        }
    }

    /// Follows a link clicked in a markdown document.
    fn follow(&mut self, link: &str, ctx: &egui::Context) {
        let Some(from) = self.path.clone() else { return };
        match links::resolve(link, &from) {
            Target::Web(url) => ctx.open_url(egui::OpenUrl::new_tab(url)),
            Target::Read(path) if path.is_file() => self.go(path),
            Target::Read(path) => self.notice = Some(format!("There's no {}", self.relative(&path))),
            Target::Open(path) => self.open_elsewhere(&path),
            Target::Stay => {}
        }
    }

    /// Follows a reference clicked in a man page.
    fn follow_man(&mut self, link: man::Link, ctx: &egui::Context) {
        match link {
            man::Link::Page(name) => match man::find(&name) {
                Some(page) => self.go(page),
                None => self.notice = Some(format!("No man page for {name}")),
            },
            man::Link::Web(url) => ctx.open_url(egui::OpenUrl::new_tab(url)),
            man::Link::Path(path) if path.is_file() && Kind::of(&path).is_some() => self.go(path),
            man::Link::Path(path) => self.open_elsewhere(&path),
        }
    }

    /// A file Reader doesn't show, or a folder, in the desktop's app for it.
    fn open_elsewhere(&mut self, path: &Path) {
        if let Err(e) = patina::open::path(path) {
            self.notice = Some(format!("Can't open {}: {e}", path.display()));
        }
    }

    /// Searches the file read again for `query`, staying at the same match
    /// rather than going back to the first.
    fn search_again(&mut self, query: &str, current: Option<usize>) {
        self.search(query);
        if !matches!(self.shown, Some(Shown::Markdown(_))) && self.found.total > 0 {
            self.found.current = current.map(|c| c.min(self.found.total - 1)).or(Some(0));
            self.found.scroll = false;
        }
    }

    /// Searches the file on screen for `query`, from the top.
    fn search(&mut self, query: &str) {
        self.found = Found::default();
        match &mut self.shown {
            Some(Shown::Markdown(editor)) => {
                editor.set_search(query);
                if let Some((at, total)) = editor.find(true) {
                    self.found = Found { current: Some(at - 1), total, ..Found::default() };
                }
            }
            Some(Shown::Man(page)) => self.found = marks(search::find_all(&page.text, query)),
            Some(Shown::Rust(text)) => self.found = marks(search::find_all(text, query)),
            _ => {}
        }
    }

    /// The next (or previous) place the search text is.
    fn step(&mut self, forward: bool) {
        if let Some(Shown::Markdown(editor)) = &mut self.shown {
            if let Some((at, total)) = editor.find(forward) {
                self.found = Found { current: Some(at - 1), total, ..Found::default() };
            }
            return;
        }
        let total = self.found.marks.len();
        if total == 0 {
            return;
        }
        let current = self.found.current.unwrap_or(0);
        self.found.current = Some(if forward { (current + 1) % total } else { (current + total - 1) % total });
        self.found.scroll = true;
    }
}

/// Found places, the first current and brought into view.
fn marks(marks: Vec<Range<usize>>) -> Found {
    let total = marks.len();
    Found { current: (total > 0).then_some(0), total, scroll: true, marks }
}

/// A markdown or source file, read from disk.
fn read(path: &Path, kind: Option<Kind>) -> Shown {
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

/// How the window was opened.
pub enum Start {
    /// Projects in tabs, those open last time, plus `root` at `file` if given.
    Tabs { root: Option<PathBuf>, file: Option<PathBuf> },
    /// One file alone, without the tree or tabs.
    Single { root: Option<PathBuf>, file: Option<PathBuf> },
    /// The system's man pages, at a page.
    Manual { page: Option<PathBuf> },
}

pub struct Reader {
    config: Config,
    skins: Vec<Skin>,
    skin: usize,
    skin_errors: Vec<String>,
    /// The latest message for the status bar.
    status: Option<Status>,
    tabs: Vec<Tab>,
    tab: usize,
    /// Single-file mode: no tree or tabs.
    single: bool,
    /// Whether the tabs are remembered: not for single files or a manual
    /// opened with --man, which help and lookups use.
    remember: bool,
    /// The search field in the title bar, and whether to focus it.
    query: String,
    focus_search: bool,
    focus_filter: bool,
    /// The name typed in Go's Man Page field.
    man_query: String,
    title: String,
    screenshot: Option<(PathBuf, bool)>,
    /// Folders watched for changes; none if the system can't watch.
    watch: Option<Watch>,
}

impl Reader {
    pub fn new(cc: &eframe::CreationContext<'_>, start: Start) -> Self {
        patina::appearance::watch(&cc.egui_ctx);
        patina::markdown::install(&cc.egui_ctx);
        let config = Config::load();
        let (skins, skin_errors) = load_skins();
        let skin = skins.iter().position(|s| s.name == config.skin).unwrap_or(0);
        let (tabs, tab, single, remember) = match start {
            Start::Single { root, file } => {
                let tab = match (root, file) {
                    (Some(root), file) => Tab::project(root, file),
                    (None, Some(file)) => {
                        let mut tab = Tab::empty();
                        tab.show(file);
                        tab
                    }
                    (None, None) => Tab::empty(),
                };
                (vec![tab], 0, true, false)
            }
            Start::Manual { page } => (vec![Tab::manual(page)], 0, false, false),
            Start::Tabs { root, file } => {
                let (mut tabs, mut tab) = restore(&config);
                if let Some(root) = root {
                    // An open tab on that project shows the file; else a new tab.
                    match tabs.iter().position(|t| t.root.as_ref() == Some(&root)) {
                        Some(i) => {
                            if let Some(file) = file {
                                tabs[i].go(file);
                            }
                            tab = i;
                        }
                        None => {
                            tabs.retain(|t| t.root.is_some() || t.man);
                            tabs.push(Tab::project(root, file));
                            tab = tabs.len() - 1;
                        }
                    }
                }
                if tabs.is_empty() {
                    tabs.push(Tab::empty());
                }
                let tab = tab.min(tabs.len() - 1);
                (tabs, tab, false, true)
            }
        };
        let mut reader = Reader {
            config,
            skins,
            skin,
            skin_errors,
            status: None,
            tabs,
            tab,
            single,
            remember,
            query: String::new(),
            focus_search: false,
            focus_filter: false,
            man_query: String::new(),
            title: String::new(),
            screenshot: std::env::var_os("READER_SCREENSHOT").map(|p| (PathBuf::from(p), false)),
            watch: Watch::new(&cc.egui_ctx).inspect_err(|e| eprintln!("reader: can't watch for changes: {e}")).ok(),
        };
        reader.apply_skin(&cc.egui_ctx);
        if reader.config.system_title_bar {
            cc.egui_ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(true));
        }
        reader.save();
        reader
    }

    fn current(&mut self) -> &mut Tab {
        &mut self.tabs[self.tab]
    }

    fn apply_skin(&self, ctx: &egui::Context) {
        let skin = &self.skins[self.skin];
        skin.apply(ctx);
        if skin.follows_desktop() {
            ctx.set_theme(self.config.theme());
        }
    }

    /// Remembers the tabs, unless this window is for help or a lookup.
    fn save(&mut self) {
        if !self.remember {
            return;
        }
        self.config.tabs = self.tabs.iter().map(Tab::saved).collect();
        self.config.tab = self.tab;
        self.config.last_root = None;
        self.config.last_file = None;
        self.config.save();
    }

    /// Puts `tab` in front: in place of an empty tab in front, else as a
    /// new tab after it.
    fn open_tab(&mut self, tab: Tab) {
        if self.tabs[self.tab].root.is_none() && !self.tabs[self.tab].man {
            self.tabs[self.tab] = tab;
        } else {
            self.tab += 1;
            self.tabs.insert(self.tab, tab);
        }
        self.searched();
        self.save();
    }

    fn pick_folder(&mut self) {
        let mut dialog = rfd::FileDialog::new().set_title("Open a Project");
        if let Some(root) = &self.tabs[self.tab].root {
            dialog = dialog.set_directory(root);
        }
        if let Some(root) = dialog.pick_folder() {
            match self.tabs.iter().position(|t| t.root.as_ref() == Some(&root)) {
                Some(i) => self.tab = i,
                None => self.open_tab(Tab::project(root, None)),
            }
            self.save();
        }
    }

    /// The manual's tab, if one is open, else a new one.
    fn open_manual(&mut self) {
        match self.tabs.iter().position(|t| t.man) {
            Some(i) => self.tab = i,
            None => self.open_tab(Tab::manual(None)),
        }
    }

    fn tab_action(&mut self, action: TabAction) {
        match action {
            TabAction::New => {
                self.tab += 1;
                self.tabs.insert(self.tab, Tab::empty());
            }
            TabAction::Show(i) => self.tab = i.min(self.tabs.len() - 1),
            TabAction::Close(i) => {
                self.tabs.remove(i);
                if self.tabs.is_empty() {
                    self.tabs.push(Tab::empty());
                }
                if i < self.tab || self.tab >= self.tabs.len() {
                    self.tab = self.tab.saturating_sub(1);
                }
            }
        }
        self.searched();
        self.save();
    }

    /// The file on screen changed (another tab, another file): search it
    /// for what's in the field.
    fn searched(&mut self) {
        let query = self.query.clone();
        if !query.is_empty() {
            self.current().search(&query);
        }
    }

    /// Goes to `path` in the tab in front, and remembers it.
    fn go(&mut self, path: PathBuf) {
        self.current().go(path);
        self.searched();
        self.save();
    }

    fn keys(&mut self, ctx: &egui::Context) {
        let (back, forward, open, reload, find, find_file, next, close, quit) = ctx.input_mut(|i| {
            let mouse_back = i.pointer.button_pressed(egui::PointerButton::Extra1);
            let mouse_forward = i.pointer.button_pressed(egui::PointerButton::Extra2);
            let next = if i.consume_key(Modifiers::SHIFT, Key::F3) {
                Some(false)
            } else if i.consume_key(Modifiers::NONE, Key::F3) {
                Some(true)
            } else {
                None
            };
            (
                i.consume_key(Modifiers::ALT, Key::ArrowLeft) || mouse_back,
                i.consume_key(Modifiers::ALT, Key::ArrowRight) || mouse_forward,
                i.consume_key(Modifiers::COMMAND, Key::O),
                i.consume_key(Modifiers::COMMAND, Key::R) || i.consume_key(Modifiers::NONE, Key::F5),
                i.consume_key(Modifiers::COMMAND, Key::F),
                i.consume_key(Modifiers::COMMAND, Key::P),
                next,
                i.consume_key(Modifiers::COMMAND, Key::W),
                i.consume_key(Modifiers::COMMAND, Key::Q),
            )
        });
        let tab = self.tab;
        if back {
            self.tabs[tab].back();
            self.searched();
            self.save();
        }
        if forward {
            self.tabs[tab].forward();
            self.searched();
            self.save();
        }
        if open {
            self.pick_folder();
        }
        if reload {
            self.tabs[tab].reload();
            self.searched();
        }
        if find {
            self.focus_search = true;
        }
        if find_file {
            self.focus_filter = true;
        }
        if let Some(forward) = next {
            self.tabs[tab].step(forward);
        }
        if close && !self.single && self.tabs.len() > 1 {
            self.tab_action(TabAction::Close(tab));
        } else if close || quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if !self.single
            && let Some(action) = tabs::keys(ctx, self.tabs.len(), self.tab)
        {
            self.tab_action(action);
        }
        if !self.single && ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::T)) {
            self.tab_action(TabAction::New);
        }
    }

    // ---- Drawing ----

    fn header(&mut self, ui: &mut Ui, style: &FrameStyle, span: egui::Rangef) {
        let title = if self.single { self.title.clone() } else { String::new() };
        let titles: Vec<String> = self.tabs.iter().map(Tab::title).collect();
        let single = self.single;
        let mut action = None;
        let (custom, shown) = (!self.config.system_title_bar, self.tab);
        // The bar's two sides each need the window: one at a time.
        let this = std::cell::RefCell::new(&mut *self);
        TitleBar::new(&title, style).custom(custom).menu_bar(true).show(
            ui,
            |ui| {
                ui.menu_button("File", |ui| this.borrow_mut().file_menu(ui));
                ui.menu_button("Go", |ui| this.borrow_mut().go_menu(ui));
                ui.menu_button("Skin", |ui| this.borrow_mut().skin_menu(ui));
                if !single {
                    action = TabStrip::new(&titles, shown, style).span(span).show(ui);
                }
            },
            |ui| this.borrow_mut().search_field(ui),
        );
        if let Some(action) = action {
            self.tab_action(action);
        }
    }

    /// The title bar's search: Enter for the next match, Shift+Enter the
    /// previous, Escape to clear; how many there are beside it.
    fn search_field(&mut self, ui: &mut Ui) {
        // The window's own colours, not the bar's: the bar sets its ink as
        // every widget's text colour, which a field's background can hide.
        let window = ui.ctx().global_style().visuals.clone();
        let field = ui
            .scope(|ui| {
                *ui.visuals_mut() = window;
                ui.add(egui::TextEdit::singleline(&mut self.query).hint_text("Search").desired_width(170.0))
            })
            .inner;
        if std::mem::take(&mut self.focus_search) {
            field.request_focus();
            if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), field.id) {
                let all = self.query.chars().count();
                state.cursor.set_char_range(Some(egui::text::CCursorRange::two(
                    egui::text::CCursor::new(0),
                    egui::text::CCursor::new(all),
                )));
                state.store(ui.ctx(), field.id);
            }
        }
        let query = self.query.clone();
        if field.changed() {
            if query.is_empty() {
                self.current().search("");
            } else {
                self.current().search(&query);
            }
        }
        if field.lost_focus() {
            let (enter, shift, escape) =
                ui.input(|i| (i.key_pressed(Key::Enter), i.modifiers.shift, i.key_pressed(Key::Escape)));
            if escape {
                self.query.clear();
                self.current().search("");
            } else if enter {
                self.current().step(!shift);
                // Keep the field, so Enter goes on finding.
                field.request_focus();
            }
        }
        let found = &self.tabs[self.tab].found;
        if !query.is_empty() {
            let text = match found.current {
                Some(i) => format!("{} of {}", i + 1, found.total),
                None => "No matches".to_string(),
            };
            ui.label(RichText::new(text).small().color(ui.visuals().weak_text_color()));
        }
    }

    fn file_menu(&mut self, ui: &mut Ui) {
        if ui.add(egui::Button::new("Open Project…").shortcut_text(weak(ui, "Ctrl+O"))).clicked() {
            self.pick_folder();
        }
        if !self.single && ui.add(egui::Button::new("New Tab").shortcut_text(weak(ui, "Ctrl+T"))).clicked() {
            self.tab_action(TabAction::New);
        }
        let has_file = self.tabs[self.tab].path.is_some();
        if ui.add_enabled(has_file, egui::Button::new("Reload").shortcut_text(weak(ui, "Ctrl+R"))).clicked() {
            self.current().reload();
            self.searched();
        }
        let mut tree = !self.single;
        if ui.add(egui::Checkbox::new(&mut tree, "Project Tree")).changed() {
            self.single = !tree;
        }
        let path = self.tabs[self.tab].path.clone();
        let folder = path.as_ref().and_then(|p| p.parent()).map(Path::to_path_buf);
        if ui.add_enabled(folder.is_some(), egui::Button::new("Show in Files")).clicked()
            && let Some(folder) = folder
        {
            let _ = patina::open::path(&folder);
        }
        if ui.add_enabled(path.is_some(), egui::Button::new("Copy Path")).clicked()
            && let Some(path) = &path
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
        if !self.single && ui.add(egui::Button::new("Close Tab").shortcut_text(weak(ui, "Ctrl+W"))).clicked() {
            self.tab_action(TabAction::Close(self.tab));
        }
        if ui.add(egui::Button::new("Quit").shortcut_text(weak(ui, "Ctrl+Q"))).clicked() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn go_menu(&mut self, ui: &mut Ui) {
        let tab = &self.tabs[self.tab];
        let (can_back, can_forward, man) = (!tab.back.is_empty(), !tab.forward.is_empty(), tab.man);
        if ui.add_enabled(can_back, egui::Button::new("Back").shortcut_text(weak(ui, "Alt+Left"))).clicked() {
            self.current().back();
            self.searched();
            self.save();
        }
        if ui.add_enabled(can_forward, egui::Button::new("Forward").shortcut_text(weak(ui, "Alt+Right"))).clicked() {
            self.current().forward();
            self.searched();
            self.save();
        }
        ui.separator();
        let readme = tree::first_file(&self.tabs[self.tab].nodes).filter(|_| !man);
        if ui.add_enabled(readme.is_some(), egui::Button::new("Project README")).clicked()
            && let Some(readme) = readme
        {
            self.go(readme);
        }
        if ui.add(egui::Button::new("System Manual")).clicked() {
            self.open_manual();
        }
        if ui.add(egui::Button::new("Find File").shortcut_text(weak(ui, "Ctrl+P"))).clicked() {
            self.focus_filter = true;
        }
        if ui.add(egui::Button::new("Search").shortcut_text(weak(ui, "Ctrl+F"))).clicked() {
            self.focus_search = true;
        }
        ui.separator();
        ui.label("Man Page:");
        let field =
            ui.add(egui::TextEdit::singleline(&mut self.man_query).hint_text("ls, printf(3)").desired_width(160.0));
        if field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
            let query = std::mem::take(&mut self.man_query);
            match man::find(&query) {
                Some(page) => self.go(page),
                None => self.current().notice = Some(format!("No man page for {query}")),
            }
            ui.close();
        }
    }

    /// Rescans the built-in and user skins, keeping the one in use by name.
    fn reload_skins(&mut self) {
        let name = self.skins[self.skin].name.clone();
        (self.skins, self.skin_errors) = load_skins();
        self.skin = self.skins.iter().position(|s| s.name == name).unwrap_or(0);
    }

    fn skin_menu(&mut self, ui: &mut Ui) {
        // Size the menu to its widest entry instead of wrapping entries to its first width.
        ui.style_mut().wrap_mode = Some(TextWrapMode::Extend);
        let ctx = ui.ctx().clone();
        let mut changed = false;
        for (i, skin) in self.skins.iter().enumerate() {
            let label = match &skin.author {
                Some(author) if skin.path.is_some() => format!("{} — {author}", skin.name),
                _ => skin.name.clone(),
            };
            let resp = ui.radio(self.skin == i, label);
            let resp = match &skin.path {
                Some(path) => resp.on_hover_text(path.display().to_string()),
                None => resp,
            };
            if resp.clicked() {
                self.skin = i;
                changed = true;
            }
        }
        ui.separator();
        ui.add_enabled_ui(self.skins[self.skin].follows_desktop(), |ui| {
            for (value, label) in [("system", "Follow desktop"), ("light", "Light"), ("dark", "Dark")] {
                if ui.radio(self.config.light_dark == value, label).clicked() {
                    self.config.light_dark = value.into();
                    changed = true;
                }
            }
        });
        ui.separator();
        if ui.button("Reload skins").clicked() {
            self.reload_skins();
            changed = true;
        }
        if let Some(dir) = skin::app_dir(APP) {
            if ui
                .button("Copy built-in skins to folder")
                .on_hover_text("Puts editable copies in the skins folder; never overwrites existing files")
                .clicked()
            {
                self.status = Some(match skin::export_built_ins(&dir, skin::BUILT_IN) {
                    Ok(export) => {
                        // The copies share the built-ins' names, so after a reload they're the
                        // versions in use.
                        self.reload_skins();
                        changed = true;
                        let mut parts = Vec::new();
                        if !export.copied.is_empty() {
                            parts.push(format!("Copied {}", export.copied.join(", ")));
                        }
                        if !export.skipped.is_empty() {
                            parts.push(format!("kept your existing {}", export.skipped.join(", ")));
                        }
                        Status::info(parts.join("; "))
                    }
                    Err(e) => Status::error(format!("Couldn't copy skins: {e}")),
                });
            }
            if ui.button("Open skins folder").on_hover_text(dir.display().to_string()).clicked() {
                let _ = std::fs::create_dir_all(&dir);
                let _ = patina::open::path(&dir);
            }
        }
        if changed {
            self.config.skin = self.skins[self.skin].name.clone();
            self.config.save();
            self.apply_skin(&ctx);
        }
    }

    /// The version on the left, then the latest message, colored by its level. Skin load errors
    /// stay until the skins load cleanly.
    fn status_bar(&self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            // Just the version: the app's name is already in the title bar and the window list, so
            // repeating it here adds nothing.
            ui.label(weak(ui, &format!("v{}", env!("CARGO_PKG_VERSION"))).small());
            let skin_errors = (!self.skin_errors.is_empty()).then(|| {
                let n = self.skin_errors.len();
                (Status::error(format!("Couldn't load {n} skin{}", if n == 1 { "" } else { "s" })), true)
            });
            if let Some((status, list)) = skin_errors.or(self.status.clone().map(|s| (s, false))) {
                ui.separator();
                let color = match status.level {
                    Level::Error => ui.visuals().error_fg_color,
                    Level::Warn => ui.visuals().warn_fg_color,
                    Level::Info => ui.visuals().weak_text_color(),
                };
                let resp = ui.label(RichText::new(status.text).color(color).small());
                if list {
                    resp.on_hover_text(self.skin_errors.join("\n"));
                }
            }
        });
    }

    fn sidebar(&mut self, ui: &mut Ui) {
        ui.add_space(8.0);
        let focus = std::mem::take(&mut self.focus_filter);
        let tab = &mut self.tabs[self.tab];
        if tab.man {
            ui.label(RichText::new("Manual").strong()).on_hover_text("Every man page on this system");
        } else if let Some(root) = &tab.root {
            ui.label(RichText::new(tab.title()).strong()).on_hover_text(root.display().to_string());
        } else {
            return;
        }
        let filter = ui.add(
            egui::TextEdit::singleline(&mut tab.filter)
                .hint_text(if tab.man { "Find a page" } else { "Find a file" })
                .desired_width(f32::INFINITY),
        );
        if focus {
            filter.request_focus();
        }
        if filter.has_focus() && ui.input(|i| i.key_pressed(Key::Escape)) {
            tab.filter.clear();
        }
        let enter = filter.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        ui.add_space(4.0);
        let nodes = tree::filtered(&tab.nodes, &tab.filter);
        let filtering = !tab.filter.trim().is_empty();
        let mut picked = None;
        if enter && filtering {
            picked = first_match(&nodes);
        }
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            draw_tree(ui, tab, &nodes, 0, filtering, &mut picked);
        });
        tab.reveal = false;
        if let Some(path) = picked {
            self.go(path);
        }
    }

    fn content(&mut self, ui: &mut Ui) {
        let tab_index = self.tab;
        let Some(path) = self.tabs[tab_index].path.clone() else {
            self.welcome(ui);
            return;
        };
        let tab = &mut self.tabs[tab_index];
        ui.horizontal(|ui| {
            ui.label(RichText::new(tab.relative(&path)).weak().small());
            if let Some(notice) = &tab.notice {
                ui.colored_label(ui.visuals().error_fg_color, RichText::new(notice).small());
            }
        });
        ui.add_space(4.0);
        let mut link = None;
        let mut man_link = None;
        let scroll = std::mem::take(&mut tab.found.scroll);
        let (marks, current) = (tab.found.marks.clone(), tab.found.current);
        match &mut tab.shown {
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
                    let shown = man::show(ui, page, Id::new(("man", &path)), &marks, current);
                    if scroll && let Some(at) = shown.current {
                        ui.scroll_to_rect(at.expand(48.0), Some(Align::Center));
                    }
                    man_link = shown.clicked;
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
                            let at = embeds::code_file(ui, text, "rs", Id::new(("source", &path)), &marks, current);
                            if scroll && let Some(at) = at {
                                ui.scroll_to_rect(at.expand(48.0), Some(Align::Center));
                            }
                        });
                    });
            }
            Some(Shown::Error(message)) => {
                ui.colored_label(ui.visuals().error_fg_color, message.as_str());
            }
            None => {}
        }
        let before = tab.path.clone();
        if let Some(link) = link {
            tab.follow(&link, ui.ctx());
        }
        if let Some(link) = man_link {
            tab.follow_man(link, ui.ctx());
        }
        if self.tabs[tab_index].path != before {
            self.searched();
            self.save();
        }
    }

    fn welcome(&mut self, ui: &mut Ui) {
        let empty_project = self.tabs[self.tab].root.is_some();
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() / 3.0);
            ui.heading("Reader");
            ui.label(RichText::new("Read a project's documentation and code, or the system's manual.").weak());
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                // Centred: the pair's width, split around it.
                let width = 260.0;
                ui.add_space(((ui.available_width() - width) / 2.0).max(0.0));
                if ui.button("Open Project…").clicked() {
                    self.pick_folder();
                }
                if ui.button("System Manual").clicked() {
                    self.open_manual();
                }
            });
            if empty_project {
                ui.add_space(8.0);
                ui.label(RichText::new("Nothing to read here: no markdown, man pages or Rust.").weak());
            }
        });
    }

    /// Watches the tabs' folders, and takes in what changed in them.
    fn watch_files(&mut self, ctx: &egui::Context) {
        let Some(watch) = &mut self.watch else { return };
        watch.set(self.tabs.iter().flat_map(Tab::watched));
        let Some(changes) = watch.changes(ctx) else { return };
        for (i, tab) in self.tabs.iter_mut().enumerate() {
            let current = tab.found.current;
            if tab.changed(&changes) && i == self.tab && !self.query.is_empty() {
                tab.search_again(&self.query, current);
            }
        }
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let tab = &self.tabs[self.tab];
        let title = match &tab.path {
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
    /// `READER_SCREENSHOT` and quits. `READER_SEARCH` searches first.
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
        } else if ctx.input(|i| i.time) > 0.5
            && self.query.is_empty()
            && let Ok(query) = std::env::var("READER_SEARCH")
        {
            self.query = query.clone();
            self.current().search(&query);
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
        self.watch_files(&ctx);
        self.update_title(&ctx);

        let header = frame::header_frame(ui.style(), &style, radius);
        egui::Panel::top("header").frame(header).show(ui, |ui| {
            // The bar's full height, padding and all, for the tabs to fill.
            let span = (ui.max_rect() + header.inner_margin).y_range();
            self.header(ui, &style, span);
        });
        egui::Panel::bottom("status")
            .frame(
                egui::Frame::side_top_panel(ui.style())
                    .inner_margin(egui::Margin::symmetric(10, 4))
                    .corner_radius(CornerRadius { sw: radius, se: radius, ..Default::default() }),
            )
            .show(ui, |ui| self.status_bar(ui));
        let has_tree = !self.single && (self.tabs[self.tab].root.is_some() || self.tabs[self.tab].man);
        if has_tree {
            egui::Panel::left("tree")
                .frame(
                    egui::Frame::side_top_panel(ui.style())
                        .inner_margin(egui::Margin::symmetric(8, 0)),
                )
                .default_size(240.0)
                .size_range(160.0..=480.0)
                .show(ui, |ui| self.sidebar(ui));
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(ui.style()).inner_margin(egui::Margin::symmetric(24, 12)))
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

/// The tabs open last time, and which was in front; a config from before
/// tabs opens its last project as one.
fn restore(config: &Config) -> (Vec<Tab>, usize) {
    let saved: Vec<SavedTab> = if config.tabs.is_empty() {
        config
            .last_root
            .clone()
            .map(|root| SavedTab { root: Some(root), man: false, file: config.last_file.clone() })
            .into_iter()
            .collect()
    } else {
        config.tabs.clone()
    };
    let tabs: Vec<Tab> = saved
        .into_iter()
        .filter_map(|t| match (t.man, t.root) {
            (true, _) => Some(Tab::manual(t.file)),
            (false, Some(root)) if root.is_dir() => Some(Tab::project(root, t.file)),
            _ => None,
        })
        .collect();
    (tabs, config.tab)
}

/// The tree's rows for `nodes`, folders opening and closing, a file
/// clicked put in `picked`.
fn draw_tree(ui: &mut Ui, tab: &mut Tab, nodes: &[Node], depth: usize, filtering: bool, picked: &mut Option<PathBuf>) {
    for node in nodes {
        match node {
            Node::Folder { name, path, children } => {
                let open = filtering || tab.expanded.contains(path);
                let resp = row(ui, depth, Mark::Folder(open), name, false);
                if resp.clicked() && !filtering && !tab.expanded.remove(path) {
                    tab.expanded.insert(path.clone());
                }
                if open {
                    draw_tree(ui, tab, children, depth + 1, filtering, picked);
                }
            }
            Node::File { name, path, kind } => {
                let current = tab.path.as_ref() == Some(path);
                let mark = match kind {
                    Kind::Markdown => "md",
                    Kind::Man => "man",
                    Kind::Rust => "rs",
                };
                let resp = row(ui, depth, Mark::Kind(mark), name, current);
                if current && tab.reveal {
                    resp.scroll_to_me(Some(Align::Center));
                }
                if resp.clicked() {
                    *picked = Some(path.clone());
                }
            }
        }
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

/// A message for the status bar.
#[derive(Clone)]
struct Status {
    level: Level,
    text: String,
}

#[derive(Clone, Copy)]
#[allow(dead_code)] // Nothing warns yet.
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

fn weak(ui: &Ui, text: &str) -> RichText {
    RichText::new(text).color(ui.visuals().weak_text_color())
}

fn load_skins() -> (Vec<Skin>, Vec<String>) {
    skin::load_all(skin::BUILT_IN, &skin::app_dir(APP).into_iter().collect::<Vec<_>>())
}

/// RGBA pixels as a PNG, for screenshot mode.
fn egui_png(out: &mut Vec<u8>, w: u32, h: u32, rgba: &[u8]) -> Result<(), String> {
    let image = image::RgbaImage::from_raw(w, h, rgba.to_vec()).ok_or("the screenshot has the wrong size")?;
    image.write_to(&mut std::io::Cursor::new(out), image::ImageFormat::Png).map_err(|e| e.to_string())
}
