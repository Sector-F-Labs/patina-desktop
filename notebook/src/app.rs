//! The window: Patina's frame with the menus, tabs and toolbar in its header
//! bar, and for the shown tab the notes sidebar and the editor (or the
//! markdown source) for the open note. Each tab is a workspace of its own.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use egui::{Key, KeyboardShortcut, Modifiers, RichText};
use patina::frame::{self, TitleBar};
use patina::ime;
use patina::skin::{self, Skin};
use patina::tabs::{self, TabAction, TabStrip};

use crate::config::Config;
use crate::editor::{BlockStyle, Clipboard, Editor};
use crate::markdown::{Image, Inline};
use crate::notes::{self, Entry};

/// How long after the last edit a note is saved.
const AUTOSAVE: f64 = 1.5;

struct OpenNote {
    path: PathBuf,
    /// The editor's edit count when last saved or loaded.
    saved_edits: u64,
    /// When the last unsaved edit happened.
    edited_at: f64,
}

/// Something asked for from the header bar, done once it's drawn.
enum Action {
    NewTab,
    CloseTab,
    PickFolder,
    NewNote,
    NewFolder,
    Refresh,
    RenameNote,
    Save,
    TrashNote,
    Quit,
    Undo,
    Redo,
    Clipboard(Clipboard),
    SelectAll,
    SetSkin(String),
    SetLightDark(String),
    ReloadSkins,
    CopySkins,
    OpenSkinsFolder,
    SystemTitleBar(bool),
    ToggleSource,
    InsertTable,
    InsertImage,
    Block(BlockStyle),
    Inline(Inline),
}

enum Dialog {
    NewNote { dir: PathBuf, name: String },
    NewFolder { dir: PathBuf, name: String },
    Rename { path: PathBuf, name: String },
    Delete { path: PathBuf },
}

/// A tab: a workspace folder, a note open on its own (ADR-0007), or
/// neither yet, with its own open note and editor.
struct Tab {
    /// Unique in the window, for the tab's widget ids.
    id: u64,
    folder: Option<PathBuf>,
    entries: Vec<Entry>,
    note: Option<OpenNote>,
    editor: Editor,
    /// The markdown source while the Markdown view is on.
    source: Option<String>,
}

pub struct App {
    config: Config,
    skins: Vec<Skin>,
    skin_errors: Vec<String>,
    tabs: Vec<Tab>,
    /// The index of the tab shown.
    shown: usize,
    next_tab_id: u64,
    /// Whether this window's tabs are the ones restored next launch: not
    /// when it was started on a path, e.g. a note from the file manager.
    keeps_tabs: bool,
    dialog: Option<Dialog>,
    dialog_error: Option<String>,
    /// The latest message for the status bar.
    status: Option<Status>,
    /// A folder being picked, and the id of the tab it's for.
    picked: Option<(u64, mpsc::Receiver<PathBuf>)>,
    /// Images being picked to insert, and the id of the tab they're for.
    picked_images: Option<(u64, mpsc::Receiver<Vec<PathBuf>>)>,
    /// Screenshot mode: where to save, and whether one was asked for.
    screenshot: Option<(PathBuf, bool)>,
    forced_skin: Option<String>,
    /// Put the keyboard in the editor once this frame's clicks are handled.
    focus_editor: bool,
}

impl Tab {
    fn new(id: u64) -> Self {
        Self {
            id,
            folder: None,
            entries: Vec::new(),
            note: None,
            editor: Editor::new(egui::Id::new(("note-editor", id))),
            source: None,
        }
    }

    /// The tab's name in the strip: its folder, else its note.
    fn title(&self) -> String {
        let dirty = if self.dirty() { " •" } else { "" };
        match (&self.folder, &self.note) {
            (Some(folder), _) => format!("{}{dirty}", notes::display_name(folder)),
            (None, Some(note)) => format!("{}{dirty}", notes::display_name(&note.path)),
            (None, None) => "New Tab".into(),
        }
    }

    fn open_folder(&mut self, folder: PathBuf) -> Result<(), String> {
        let saved = self.save();
        self.entries = notes::scan(&folder);
        let first = notes::first_note(&self.entries);
        self.folder = Some(folder);
        let opened = match first {
            Some(path) => self.open_note(path),
            None => self.close_note(),
        };
        saved.and(opened)
    }

    /// Re-read the workspace from disk, for changes made outside the app.
    fn refresh(&mut self) {
        if let Some(folder) = &self.folder {
            self.entries = notes::scan(folder);
        }
    }

    fn open_note(&mut self, path: PathBuf) -> Result<(), String> {
        if self.note.as_ref().is_some_and(|n| n.path == path) {
            return Ok(());
        }
        let saved = self.save();
        let text = std::fs::read_to_string(&path).map_err(|e| format!("Could not open {}: {e}", path.display()))?;
        self.editor.load(&text);
        self.editor.set_dir(path.parent().map(Path::to_path_buf));
        self.source = self.source.is_some().then_some(text);
        self.note = Some(OpenNote { path, saved_edits: self.editor.edits(), edited_at: 0.0 });
        saved
    }

    fn close_note(&mut self) -> Result<(), String> {
        let saved = self.save();
        self.note = None;
        self.editor.load("");
        self.editor.set_dir(None);
        self.source = None;
        saved
    }

    fn dirty(&self) -> bool {
        self.note.as_ref().is_some_and(|n| n.saved_edits != self.editor.edits())
    }

    /// Writes the open note if it changed since it was loaded or saved.
    fn save(&mut self) -> Result<(), String> {
        if !self.dirty() {
            return Ok(());
        }
        let Some(note) = &mut self.note else { return Ok(()) };
        let text = match &self.source {
            Some(source) => source.clone(),
            None => self.editor.markdown(),
        };
        notes::write_atomic(&note.path, &text).map_err(|e| format!("Could not save {}: {e}", note.path.display()))?;
        note.saved_edits = self.editor.edits();
        Ok(())
    }

    fn create_note(&mut self, dir: &Path, name: &str) -> Result<(), String> {
        notes::valid_name(name)?;
        let path = dir.join(notes::note_file_name(name));
        if path.exists() {
            return Err(format!("{} already exists", notes::display_name(&path)));
        }
        let title = notes::display_name(&path);
        notes::write_atomic(&path, &format!("# {title}\n")).map_err(|e| e.to_string())?;
        self.refresh();
        self.open_note(path)?;
        self.editor.cursor_to_first_line_end();
        Ok(())
    }

    fn create_folder(&mut self, dir: &Path, name: &str) -> Result<(), String> {
        notes::valid_name(name)?;
        std::fs::create_dir(dir.join(name.trim())).map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    /// Renames or moves a note or folder, following the open note.
    fn move_entry(&mut self, from: &Path, to: PathBuf) -> Result<(), String> {
        if to.exists() {
            return Err(format!("{} already exists", to.display()));
        }
        if to.starts_with(from) {
            return Err("A folder can't go inside itself".into());
        }
        self.save()?;
        std::fs::rename(from, &to).map_err(|e| e.to_string())?;
        if let Some(note) = &mut self.note {
            note.path = notes::moved_path(&note.path, from, &to);
            self.editor.set_dir(note.path.parent().map(Path::to_path_buf));
        }
        self.refresh();
        Ok(())
    }

    /// Adds image files to the open note at the cursor. One already in the
    /// workspace is linked where it is; any other is copied next to the
    /// note first, so the note doesn't depend on it (ADR-0012).
    fn add_images(&mut self, files: &[PathBuf], now: f64) -> Result<(), String> {
        let Some(dir) = self.note.as_ref().and_then(|n| n.path.parent()).map(Path::to_path_buf) else {
            return Err("Open a note to add images to".into());
        };
        if self.source.is_some() {
            return Err("Switch off the Markdown view to add images".into());
        }
        let root = self.folder.clone().unwrap_or_else(|| dir.clone());
        for file in files {
            let file = file.canonicalize().unwrap_or_else(|_| file.clone());
            let target = if file.starts_with(&root) {
                file.clone()
            } else {
                let name = file.file_name().unwrap_or_default().to_string_lossy();
                let target = notes::free_path(&dir, &name);
                std::fs::copy(&file, &target).map_err(|e| format!("Could not copy {}: {e}", file.display()))?;
                target
            };
            let alt = file.file_stem().unwrap_or_default().to_string_lossy().into_owned();
            let src = notes::relative_src(&dir, &target);
            self.editor.insert_image(Image { alt, src, title: String::new(), html: None }, now);
        }
        Ok(())
    }

    fn rename(&mut self, path: &Path, name: &str) -> Result<(), String> {
        notes::valid_name(name)?;
        let file = if path.is_dir() { name.trim().to_string() } else { notes::note_file_name(name) };
        let to = path.with_file_name(file);
        if to == path {
            return Ok(());
        }
        self.move_entry(path, to)
    }

    fn delete(&mut self, path: &Path) -> Result<(), String> {
        if self.note.as_ref().is_some_and(|n| n.path.starts_with(path)) {
            self.close_note()?;
        }
        trash::delete(path).map_err(|e| e.to_string())?;
        self.refresh();
        Ok(())
    }

    fn toggle_source(&mut self) {
        match self.source.take() {
            Some(source) => {
                let dirty = self.dirty();
                self.editor.load(&source);
                if let Some(note) = &mut self.note {
                    // Loading resets the count; keep unsaved changes unsaved.
                    note.saved_edits = if dirty { u64::MAX } else { self.editor.edits() };
                }
            }
            None => self.source = Some(self.editor.markdown()),
        }
    }

    /// Where a new note or folder goes: the open note's folder, else the
    /// workspace.
    fn target_dir(&self) -> Option<PathBuf> {
        let folder = self.folder.as_ref()?;
        Some(
            self.note
                .as_ref()
                .and_then(|n| n.path.parent())
                .filter(|p| p.starts_with(folder))
                .map_or_else(|| folder.clone(), Path::to_path_buf),
        )
    }
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, open: Option<PathBuf>) -> Self {
        patina::markdown::install(&cc.egui_ctx);
        patina::appearance::watch(&cc.egui_ctx);
        let config = Config::load();
        let (skins, skin_errors) = load_skins();
        let mut app = Self {
            config,
            skins,
            skin_errors,
            tabs: Vec::new(),
            shown: 0,
            next_tab_id: 0,
            keeps_tabs: open.is_none(),
            dialog: None,
            dialog_error: None,
            status: None,
            picked: None,
            picked_images: None,
            screenshot: std::env::var_os("NOTEBOOK_SCREENSHOT").map(|p| (PathBuf::from(p), false)),
            forced_skin: None,
            focus_editor: false,
        };
        // For screenshots: a skin for this run only, not saved.
        app.forced_skin = std::env::var("NOTEBOOK_SKIN").ok();
        app.apply_skin(&cc.egui_ctx);
        match std::env::var("NOTEBOOK_THEME").as_deref() {
            Ok("light") => cc.egui_ctx.set_theme(egui::ThemePreference::Light),
            Ok("dark") => cc.egui_ctx.set_theme(egui::ThemePreference::Dark),
            _ => {}
        }
        if open.is_some() {
            app.new_tab();
        }
        match open {
            Some(path) if path.is_dir() => app.open_folder(path),
            Some(path) => {
                // A note inside the remembered workspace opens there;
                // anything else opens on its own.
                if let Some(folder) = app.config.folder.clone().filter(|f| path.starts_with(f)) {
                    app.open_folder(folder);
                }
                app.open_note(path);
            }
            None => {
                // The tabs open last time, or the GTK Notebook's one folder.
                let mut folders: Vec<PathBuf> = app.config.tabs.iter().filter(|f| f.is_dir()).cloned().collect();
                if folders.is_empty() {
                    folders.extend(app.config.folder.clone().filter(|f| f.is_dir()));
                }
                for folder in folders {
                    app.new_tab();
                    app.open_folder(folder);
                }
                app.shown = app.config.tab.min(app.tabs.len().saturating_sub(1));
            }
        }
        if app.tabs.is_empty() {
            app.new_tab();
        }
        app
    }

    fn skin(&self) -> &Skin {
        let name = self.forced_skin.as_deref().or(self.config.skin.as_deref()).unwrap_or(HOME_SKIN);
        self.skins.iter().find(|s| s.name == name).unwrap_or(&self.skins[0])
    }

    fn apply_skin(&self, ctx: &egui::Context) {
        let skin = self.skin();
        skin.apply(ctx);
        if skin.follows_desktop() {
            ctx.set_theme(self.config.theme());
        }
    }

    fn tab(&self) -> &Tab {
        &self.tabs[self.shown]
    }

    fn tab_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.shown]
    }

    fn report(&mut self, result: Result<(), String>) {
        if let Err(e) = result {
            self.status = Some(Status::error(e));
        }
    }

    // -----------------------------------------------------------------------
    // Tabs
    // -----------------------------------------------------------------------

    /// Adds a blank tab and shows it.
    fn new_tab(&mut self) {
        self.tabs.push(Tab::new(self.next_tab_id));
        self.next_tab_id += 1;
        self.show_tab(self.tabs.len() - 1);
    }

    fn show_tab(&mut self, index: usize) {
        if index >= self.tabs.len() {
            return;
        }
        // Only the shown tab is edited, so saving here leaves no other dirty.
        if let Some(tab) = self.tabs.get_mut(self.shown) {
            let saved = tab.save();
            self.report(saved);
        }
        self.shown = index;
        self.tab_mut().refresh();
        self.focus_editor = self.tab().note.is_some();
    }

    /// Closes a tab, saving its note first. The last tab is never closed:
    /// it's replaced by a blank one, so the window always has a sidebar.
    fn close_tab(&mut self, index: usize) {
        let Some(tab) = self.tabs.get_mut(index) else { return };
        // Keep the tab if its note couldn't be saved, so nothing is lost.
        if let Err(e) = tab.save() {
            self.status = Some(Status::error(e));
            return;
        }
        self.tabs.remove(index);
        if self.tabs.is_empty() {
            self.new_tab();
            return;
        }
        if index < self.shown || self.shown == self.tabs.len() {
            self.shown -= 1;
        }
        self.tab_mut().refresh();
        self.focus_editor = self.tab().note.is_some();
    }

    fn tab_action(&mut self, action: TabAction) {
        match action {
            TabAction::New => self.new_tab(),
            TabAction::Show(i) => self.show_tab(i),
            TabAction::Close(i) => self.close_tab(i),
        }
    }

    /// Records the open tabs' folders in the config when they change.
    fn remember_tabs(&mut self) {
        if !self.keeps_tabs {
            return;
        }
        let folders: Vec<PathBuf> = self.tabs.iter().filter_map(|t| t.folder.clone()).collect();
        // The shown tab's place among those with a folder.
        let shown = self.tabs[..self.shown].iter().filter(|t| t.folder.is_some()).count();
        if folders != self.config.tabs || shown != self.config.tab {
            self.config.tabs = folders;
            self.config.tab = shown;
            self.config.save();
        }
    }

    // -----------------------------------------------------------------------
    // Files
    // -----------------------------------------------------------------------

    /// Opens `folder` in the shown tab, and remembers it (ADR-0007).
    fn open_folder(&mut self, folder: PathBuf) {
        let opened = self.tab_mut().open_folder(folder.clone());
        self.report(opened);
        self.config.folder = Some(folder);
        self.config.save();
        self.focus_editor = self.tab().note.is_some();
    }

    fn open_note(&mut self, path: PathBuf) {
        let opened = self.tab_mut().open_note(path);
        // The cursor goes in at the start, ready to type.
        self.focus_editor = opened.is_ok();
        self.report(opened);
    }

    fn pick_folder(&mut self) {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            if let Some(path) = rfd::FileDialog::new().set_title("Open Folder").pick_folder() {
                let _ = tx.send(path);
            }
        });
        self.picked = Some((self.tab().id, rx));
    }

    fn pick_images(&mut self) {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let dialog = rfd::FileDialog::new()
                .set_title("Insert Image")
                .add_filter("Images", &["png", "jpg", "jpeg", "gif", "webp", "bmp"]);
            if let Some(paths) = dialog.pick_files() {
                let _ = tx.send(paths);
            }
        });
        self.picked_images = Some((self.tab().id, rx));
    }

    /// Images to add to tab `id`'s note, if it's still open.
    fn picked_images(&mut self, ctx: &egui::Context, id: u64, files: Vec<PathBuf>) {
        let Some(i) = self.tabs.iter().position(|t| t.id == id) else { return };
        self.show_tab(i);
        self.add_images(ctx, &files);
    }

    fn add_images(&mut self, ctx: &egui::Context, files: &[PathBuf]) {
        let now = ctx.input(|i| i.time);
        let added = self.tab_mut().add_images(files, now);
        if added.is_ok() {
            let tab = self.tab_mut();
            if let Some(note) = &mut tab.note {
                note.edited_at = now;
            }
            tab.editor.focus(ctx);
        }
        self.report(added);
    }

    /// Files dropped on the window: images go into the open note, notes
    /// open in the shown tab.
    fn dropped(&mut self, ctx: &egui::Context) {
        let files: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        let images: Vec<PathBuf> = files.iter().filter(|f| notes::is_image(f)).cloned().collect();
        if !images.is_empty() {
            self.add_images(ctx, &images);
        }
        if let Some(note) = files.into_iter().find(|f| notes::is_note(f)) {
            self.open_note(note);
        }
    }

    /// A folder picked for tab `id`: opens it there, or in a new tab if
    /// that one was closed meanwhile.
    fn picked_folder(&mut self, id: u64, folder: PathBuf) {
        match self.tabs.iter().position(|t| t.id == id) {
            Some(i) => self.show_tab(i),
            None => self.new_tab(),
        }
        self.open_folder(folder);
    }

    // -----------------------------------------------------------------------
    // Shortcuts
    // -----------------------------------------------------------------------

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let cmd = Modifiers::COMMAND;
        let cmd_shift = Modifiers::COMMAND | Modifiers::SHIFT;
        let pressed = |mods, key| ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(mods, key)));
        if pressed(cmd, Key::O) {
            self.pick_folder();
        }
        if pressed(cmd_shift, Key::N) {
            self.request_new_folder(None);
        }
        if pressed(cmd, Key::N) {
            self.request_new_note(None);
        }
        if pressed(cmd, Key::S) {
            let saved = self.tab_mut().save();
            self.report(saved);
        }
        if pressed(Modifiers::NONE, Key::F5) || pressed(cmd, Key::R) {
            self.tab_mut().refresh();
        }
        if pressed(cmd, Key::Q) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if pressed(cmd_shift, Key::M) && self.tab().note.is_some() {
            self.tab_mut().toggle_source();
        }
        if pressed(Modifiers::NONE, Key::F2)
            && let Some(note) = &self.tab().note
        {
            self.dialog = Some(Dialog::Rename { name: notes::display_name(&note.path), path: note.path.clone() });
        }

        // Formatting, only while the note text has the keyboard.
        let tab = &mut self.tabs[self.shown];
        if tab.source.is_some() || !tab.editor.has_focus(ctx) {
            return;
        }
        let now = ctx.input(|i| i.time);
        let inline = [
            (cmd, Key::B, Inline::Bold),
            (cmd, Key::I, Inline::Italic),
            (cmd, Key::E, Inline::Code),
            (cmd_shift, Key::X, Inline::Strike),
        ];
        for (mods, key, style) in inline {
            if ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(mods, key))) {
                tab.editor.toggle_inline(style, now);
            }
        }
        let blocks = [
            (cmd, Key::Num0, BlockStyle::Paragraph),
            (cmd, Key::Num1, BlockStyle::Heading(1)),
            (cmd, Key::Num2, BlockStyle::Heading(2)),
            (cmd, Key::Num3, BlockStyle::Heading(3)),
            (cmd_shift, Key::Num8, BlockStyle::Bullet),
            (cmd_shift, Key::Num7, BlockStyle::Ordered),
            (cmd_shift, Key::L, BlockStyle::Task),
        ];
        for (mods, key, style) in blocks {
            if ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(mods, key))) {
                tab.editor.set_block(style, now);
            }
        }
    }

    fn request_new_note(&mut self, dir: Option<PathBuf>) {
        if let Some(dir) = dir.or_else(|| self.tab().target_dir()) {
            self.dialog = Some(Dialog::NewNote { dir, name: String::new() });
            self.dialog_error = None;
        }
    }

    fn request_new_folder(&mut self, dir: Option<PathBuf>) {
        if let Some(dir) = dir.or_else(|| self.tab().target_dir()) {
            self.dialog = Some(Dialog::NewFolder { dir, name: String::new() });
            self.dialog_error = None;
        }
    }

    // -----------------------------------------------------------------------
    // UI
    // -----------------------------------------------------------------------

    /// The title bar: menus and tabs on the left, the formatting toolbar on
    /// the right. `span` is the bar's full height, for the tabs.
    fn header(&mut self, ui: &mut egui::Ui, style: &frame::FrameStyle, span: egui::Rangef) {
        let custom = !self.config.system_title_bar;
        let titles: Vec<String> = self.tabs.iter().map(Tab::title).collect();
        let tab = self.tab();
        let editing = tab.note.is_some() && tab.source.is_none();
        let mut left = Vec::new();
        let mut right = Vec::new();
        let mut tab_action = None;
        // The tabs take the title's place (Patina ADR-0009).
        TitleBar::new("", style).custom(custom).menu_bar(true).span(span).show(
            ui,
            |ui| {
                self.menus(ui, &mut left);
                tab_action = TabStrip::new(&titles, self.shown, style).span(span).show(ui);
            },
            |ui| {
                if tab.note.is_none() {
                    return;
                }
                if ui
                    .selectable_label(tab.source.is_some(), "Markdown")
                    .on_hover_text("Show Markdown Source (Ctrl+Shift+M)")
                    .clicked()
                {
                    right.push(Action::ToggleSource);
                }
                ui.add_enabled_ui(editing, |ui| {
                    ui.separator();
                    if ui.button("🖼").on_hover_text("Insert Image…").clicked() {
                        right.push(Action::InsertImage);
                    }
                    if ui.button("⊞").on_hover_text("Insert Table").clicked() {
                        right.push(Action::InsertTable);
                    }
                    for (style, label, tip) in [
                        (BlockStyle::Task, "☑", "Checklist (Ctrl+Shift+L)"),
                        (BlockStyle::Ordered, "1.", "Numbered List (Ctrl+Shift+7)"),
                        (BlockStyle::Bullet, "•", "Bulleted List (Ctrl+Shift+8)"),
                    ] {
                        let on = editing && tab.editor.block_at_cursor() == style;
                        if ui.selectable_label(on, label).on_hover_text(tip).clicked() {
                            right.push(Action::Block(style));
                        }
                    }
                    ui.separator();
                    for (style, label, tip) in [
                        (Inline::Code, RichText::new("<>").monospace(), "Code (Ctrl+E)"),
                        (Inline::Strike, RichText::new("S").strikethrough(), "Strikethrough (Ctrl+Shift+X)"),
                        (Inline::Italic, RichText::new("I").italics(), "Italic (Ctrl+I)"),
                        (Inline::Bold, RichText::new("B").strong(), "Bold (Ctrl+B)"),
                    ] {
                        let on = editing && tab.editor.inline_active(&style);
                        if ui.selectable_label(on, label).on_hover_text(tip).clicked() {
                            right.push(Action::Inline(style));
                        }
                    }
                    ui.menu_button("Aa", |ui| {
                        for (style, label) in [
                            (BlockStyle::Heading(1), "Title"),
                            (BlockStyle::Heading(2), "Heading"),
                            (BlockStyle::Heading(3), "Subheading"),
                            (BlockStyle::Paragraph, "Body"),
                            (BlockStyle::Quote, "Quote"),
                            (BlockStyle::Code, "Code Block"),
                        ] {
                            if ui.button(label).clicked() {
                                right.push(Action::Block(style));
                                ui.close();
                            }
                        }
                    })
                    .response
                    .on_hover_text("Paragraph Style");
                });
            },
        );
        for action in left.into_iter().chain(right) {
            self.act(ui.ctx(), action);
        }
        if let Some(action) = tab_action {
            self.tab_action(action);
        }
    }

    fn act(&mut self, ctx: &egui::Context, action: Action) {
        let now = ctx.input(|i| i.time);
        match action {
            Action::NewTab => self.new_tab(),
            Action::CloseTab => self.close_tab(self.shown),
            Action::PickFolder => self.pick_folder(),
            Action::NewNote => self.request_new_note(None),
            Action::NewFolder => self.request_new_folder(None),
            Action::Refresh => self.tab_mut().refresh(),
            Action::RenameNote => {
                if let Some(note) = &self.tab().note {
                    self.dialog =
                        Some(Dialog::Rename { name: notes::display_name(&note.path), path: note.path.clone() });
                    self.dialog_error = None;
                }
            }
            Action::Save => {
                let saved = self.tab_mut().save();
                self.report(saved);
            }
            Action::TrashNote => {
                if let Some(note) = &self.tab().note {
                    self.dialog = Some(Dialog::Delete { path: note.path.clone() });
                }
            }
            Action::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Action::Undo => self.tab_mut().editor.undo(),
            Action::Redo => self.tab_mut().editor.redo(),
            Action::Clipboard(c) => self.tab_mut().editor.clipboard(ctx, c),
            Action::SelectAll => {
                let editor = &mut self.tab_mut().editor;
                editor.select_all();
                editor.focus(ctx);
            }
            Action::CopySkins => match skin::app_dir("notebook").ok_or("No config folder".to_string()).and_then(|dir| {
                skin::export_built_ins(&dir, skin::BUILT_IN)
            }) {
                Ok(export) => {
                    // The copies share the built-ins' names, so after a reload they're the
                    // versions in use.
                    (self.skins, self.skin_errors) = load_skins();
                    self.apply_skin(ctx);
                    self.status = Some(Status::info(if export.copied.is_empty() {
                        "The built-in skins are already in the skins folder".into()
                    } else {
                        format!("Copied {} to the skins folder", export.copied.join(", "))
                    }));
                }
                Err(e) => self.status = Some(Status::error(format!("Could not copy skins: {e}"))),
            },
            Action::OpenSkinsFolder => {
                if let Some(dir) = skin::app_dir("notebook") {
                    let _ = std::fs::create_dir_all(&dir);
                    let _ = patina::open::path(&dir);
                }
            }
            Action::SetSkin(name) => {
                self.forced_skin = None;
                self.config.skin = Some(name);
                self.config.save();
                self.apply_skin(ctx);
            }
            Action::SetLightDark(value) => {
                self.config.light_dark = value;
                self.config.save();
                self.apply_skin(ctx);
            }
            Action::ReloadSkins => {
                (self.skins, self.skin_errors) = load_skins();
                self.apply_skin(ctx);
            }
            Action::SystemTitleBar(on) => {
                self.config.system_title_bar = on;
                self.config.save();
                ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(on));
            }
            Action::ToggleSource => self.tab_mut().toggle_source(),
            Action::InsertTable => self.tab_mut().editor.insert_table(now),
            Action::InsertImage => self.pick_images(),
            Action::Block(style) => {
                let editor = &mut self.tab_mut().editor;
                editor.set_block(style, now);
                editor.focus(ctx);
            }
            Action::Inline(style) => {
                let editor = &mut self.tab_mut().editor;
                editor.toggle_inline(style, now);
                editor.focus(ctx);
            }
        }
    }

    /// The menu bar: File, Edit, View and Skins.
    fn menus(&self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let tab = self.tab();
        let has_folder = tab.folder.is_some();
        let has_note = tab.note.is_some();
        let editing = has_note && tab.source.is_none();
        let mut item = |ui: &mut egui::Ui, enabled: bool, label: &str, keys: &str, action: Action| {
            if ui.add_enabled(enabled, egui::Button::new(label).shortcut_text(keys)).clicked() {
                actions.push(action);
                ui.close();
            }
        };
        ui.menu_button("File", |ui| {
            item(ui, true, "New Tab", "Ctrl+T", Action::NewTab);
            item(ui, true, "Open Folder…", "Ctrl+O", Action::PickFolder);
            ui.separator();
            item(ui, has_folder, "New Note…", "Ctrl+N", Action::NewNote);
            item(ui, has_folder, "New Folder…", "Ctrl+Shift+N", Action::NewFolder);
            item(ui, has_folder, "Refresh", "F5", Action::Refresh);
            ui.separator();
            item(ui, has_note, "Save", "Ctrl+S", Action::Save);
            item(ui, has_note, "Rename Note…", "F2", Action::RenameNote);
            item(ui, has_note, "Move Note to Trash…", "", Action::TrashNote);
            ui.separator();
            item(ui, true, "Close Tab", "Ctrl+W", Action::CloseTab);
            item(ui, true, "Quit", "Ctrl+Q", Action::Quit);
        });
        ui.menu_button("Edit", |ui| {
            item(ui, editing, "Undo", "Ctrl+Z", Action::Undo);
            item(ui, editing, "Redo", "Ctrl+Shift+Z", Action::Redo);
            ui.separator();
            item(ui, editing, "Cut", "Ctrl+X", Action::Clipboard(Clipboard::Cut));
            item(ui, editing, "Copy", "Ctrl+C", Action::Clipboard(Clipboard::Copy));
            item(ui, editing, "Paste", "Ctrl+V", Action::Clipboard(Clipboard::Paste));
            ui.separator();
            item(ui, editing, "Select All", "Ctrl+A", Action::SelectAll);
            ui.separator();
            item(ui, editing, "Code Block", "", Action::Block(BlockStyle::Code));
            item(ui, editing, "Insert Table", "", Action::InsertTable);
            item(ui, editing, "Insert Image…", "", Action::InsertImage);
        });
        ui.menu_button("View", |ui| {
            let mut source = tab.source.is_some();
            if ui.add_enabled(has_note, egui::Checkbox::new(&mut source, "Markdown source")).changed() {
                actions.push(Action::ToggleSource);
            }
            let mut system = self.config.system_title_bar;
            if ui.checkbox(&mut system, "System title bar").changed() {
                actions.push(Action::SystemTitleBar(system));
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
                    actions.push(Action::SetSkin(skin.name.clone()));
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
                        actions.push(Action::SetLightDark(value.into()));
                    }
                }
            });
            ui.separator();
            if ui.button("Reload skins").clicked() {
                actions.push(Action::ReloadSkins);
            }
            let folder = skin::app_dir("notebook").map(|d| d.display().to_string()).unwrap_or_default();
            if ui
                .button("Copy built-in skins to folder")
                .on_hover_text("Puts editable copies in the skins folder; never overwrites existing files")
                .clicked()
            {
                actions.push(Action::CopySkins);
            }
            if ui.button("Open skins folder").on_hover_text(folder).clicked() {
                actions.push(Action::OpenSkinsFolder);
            }
        });
    }

    /// The version on the left, then the latest message, coloured by its level. Skins that failed
    /// to load stay there, ahead of newer messages, until they load.
    fn status_bar(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            // Just the version: the app's name is already in the title bar and the window list,
            // so repeating it here adds nothing.
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
        });
    }

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        let Some(folder) = self.tab().folder.clone() else {
            ui.add_space(8.0);
            match self.tab().note.as_ref().map(|n| n.path.parent().map(Path::to_path_buf)) {
                // A note open on its own (ADR-0007).
                Some(dir) => {
                    ui.label(RichText::new("This note is open on its own.").weak());
                    if let Some(dir) = dir
                        && ui.button("Open Containing Folder").clicked()
                    {
                        self.open_folder(dir);
                    }
                    if let Some(remembered) = self.config.folder.clone().filter(|f| f.is_dir()) {
                        let name = notes::display_name(&remembered);
                        if ui.button(format!("Back to {name}")).clicked() {
                            self.open_folder(remembered);
                        }
                    }
                }
                // A new tab, or the first run.
                None => {
                    ui.label(RichText::new("No folder open").weak());
                    ui.label(RichText::new("Open a folder of markdown notes to see them here.").weak().small());
                }
            }
            ui.add_space(4.0);
            if ui.button("Open Folder…").on_hover_text("Open Folder (Ctrl+O)").clicked() {
                self.pick_folder();
            }
            return;
        };
        let root_name = notes::display_name(&folder);
        let resp = ui.add(egui::Label::new(RichText::new(root_name).strong()).sense(egui::Sense::click()));
        patina::menu::context(&resp, |ui| self.folder_menu(ui, &folder));
        self.drop_target(ui, &resp, &folder);
        let entries = self.tab().entries.clone();
        egui::ScrollArea::vertical().id_salt(("sidebar", self.tab().id)).show(ui, |ui| {
            let new_note = ui
                .add(egui::Button::new(RichText::new("+ New Note").weak()).frame(false))
                .on_hover_text("New Note (Ctrl+N), in the open note's folder");
            if new_note.clicked() {
                self.request_new_note(None);
            }
            for entry in &entries {
                self.entry(ui, entry);
            }
            // Room below the tree for right-clicking and dropping at the top level.
            let rest = ui.available_size();
            let (_, resp) = ui.allocate_exact_size(egui::vec2(rest.x, rest.y.max(40.0)), egui::Sense::click());
            patina::menu::context(&resp, |ui| self.folder_menu(ui, &folder));
            self.drop_target(ui, &resp, &folder);
        });
    }

    /// Makes `resp` accept dragged notes and folders into `dir`,
    /// highlighting it while one hovers.
    fn drop_target(&mut self, ui: &egui::Ui, resp: &egui::Response, dir: &Path) {
        if resp.dnd_hover_payload::<PathBuf>().is_some() {
            let v = ui.visuals();
            ui.painter().rect_stroke(resp.rect.expand(1.0), 3.0, v.selection.stroke, egui::StrokeKind::Outside);
        }
        if let Some(from) = resp.dnd_release_payload::<PathBuf>() {
            self.drop_into(&from, dir);
        }
    }

    fn drop_into(&mut self, from: &Path, dir: &Path) {
        if from.parent() == Some(dir) {
            return;
        }
        let to = dir.join(from.file_name().unwrap_or_default());
        if let Err(e) = self.tab_mut().move_entry(from, to) {
            self.status = Some(Status::error(format!("Could not move {}: {e}", notes::display_name(from))));
        }
    }

    fn entry(&mut self, ui: &mut egui::Ui, entry: &Entry) {
        match entry {
            Entry::Note { path, name } => {
                let current = self.tab().note.as_ref().is_some_and(|n| &n.path == path);
                // One widget that both clicks and drags: a drag-source wrapper
                // around the label would sit on top of it and eat its clicks.
                let resp = ui.add(egui::Button::selectable(current, format!("📄 {name}")).sense(egui::Sense::click_and_drag()));
                resp.dnd_set_drag_payload(path.clone());
                if resp.clicked() {
                    self.open_note(path.clone());
                }
                patina::menu::context(&resp, |ui| self.item_menu(ui, path));
            }
            Entry::Folder { path, name, children } => {
                let id = ui.make_persistent_id(("folder", path));
                let state = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false);
                let header = state.show_header(ui, |ui| {
                    let resp = ui.add(egui::Label::new(format!("📁 {name}")).sense(egui::Sense::click_and_drag()));
                    resp.dnd_set_drag_payload(path.clone());
                    resp
                });
                let (_, header, _) = header.body(|ui| {
                    for child in children {
                        self.entry(ui, child);
                    }
                });
                let row = header.inner;
                if row.clicked() {
                    let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false);
                    state.toggle(ui);
                    state.store(ui.ctx());
                }
                patina::menu::context(&row, |ui| self.folder_menu(ui, path));
                self.drop_target(ui, &row, path);
                // Resting a drag on a closed folder opens it.
                let dragging = egui::DragAndDrop::has_payload_of_type::<PathBuf>(ui.ctx());
                if dragging && row.contains_pointer() {
                    let mut state =
                        egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false);
                    if !state.is_open() {
                        state.set_open(true);
                        state.store(ui.ctx());
                    }
                }
            }
        }
    }

    fn item_menu(&mut self, ui: &mut egui::Ui, path: &Path) {
        if ui.button("Rename…").clicked() {
            self.dialog = Some(Dialog::Rename { name: notes::display_name(path), path: path.to_path_buf() });
            self.dialog_error = None;
            ui.close();
        }
        if ui.button("Move to Trash").clicked() {
            self.dialog = Some(Dialog::Delete { path: path.to_path_buf() });
            ui.close();
        }
    }

    fn folder_menu(&mut self, ui: &mut egui::Ui, dir: &Path) {
        if ui.button("New Note…").clicked() {
            self.request_new_note(Some(dir.to_path_buf()));
            ui.close();
        }
        if ui.button("New Folder…").clicked() {
            self.request_new_folder(Some(dir.to_path_buf()));
            ui.close();
        }
        if Some(dir) != self.tab().folder.as_deref() {
            ui.separator();
            self.item_menu(ui, dir);
        } else {
            ui.separator();
            if ui.button("Refresh").clicked() {
                self.tab_mut().refresh();
                ui.close();
            }
        }
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.dialog else { return };
        let mut action: Option<bool> = None;
        let title = match dialog {
            Dialog::NewNote { .. } => "New Note",
            Dialog::NewFolder { .. } => "New Folder",
            Dialog::Rename { .. } => "Rename",
            Dialog::Delete { .. } => "Move to Trash?",
        };
        let error = self.dialog_error.clone();
        let modal = egui::Modal::new(egui::Id::new("dialog")).show(ctx, |ui| {
            ui.set_width(320.0);
            ui.heading(title);
            ui.add_space(8.0);
            match dialog {
                Dialog::NewNote { name, .. } | Dialog::NewFolder { name, .. } | Dialog::Rename { name, .. } => {
                    let resp = ui.add(egui::TextEdit::singleline(name).hint_text("Name").desired_width(f32::INFINITY));
                    if !resp.has_focus() && !resp.lost_focus() {
                        resp.request_focus();
                    }
                    if resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        action = Some(true);
                    }
                }
                Dialog::Delete { path } => {
                    ui.label(format!("“{}” will be moved to the trash.", notes::display_name(path)));
                }
            }
            if let Some(error) = &error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let ok = if matches!(dialog, Dialog::Delete { .. }) { "Move to Trash" } else { "OK" };
                if ui.button(ok).clicked() {
                    action = Some(true);
                }
                if ui.button("Cancel").clicked() {
                    action = Some(false);
                }
            });
        });
        if modal.should_close() && action.is_none() {
            action = Some(false);
        }
        match action {
            Some(false) => self.dialog = None,
            Some(true) => {
                let result = match self.dialog.take() {
                    Some(Dialog::NewNote { dir, name }) => {
                        let r = self.tab_mut().create_note(&dir, &name);
                        if r.is_ok() {
                            self.tab().editor.focus(ctx);
                        }
                        r.map_err(|e| (Dialog::NewNote { dir, name }, e))
                    }
                    Some(Dialog::NewFolder { dir, name }) => {
                        self.tab_mut().create_folder(&dir, &name).map_err(|e| (Dialog::NewFolder { dir, name }, e))
                    }
                    Some(Dialog::Rename { path, name }) => {
                        self.tab_mut().rename(&path, &name).map_err(|e| (Dialog::Rename { path, name }, e))
                    }
                    Some(Dialog::Delete { path }) => self.tab_mut().delete(&path).map_err(|e| (Dialog::Delete { path }, e)),
                    None => Ok(()),
                };
                match result {
                    Ok(()) => self.dialog_error = None,
                    Err((dialog, e)) => {
                        self.dialog = Some(dialog);
                        self.dialog_error = Some(e);
                    }
                }
            }
            None => {}
        }
    }

    fn welcome(ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() / 3.0);
            ui.heading("Notebook");
            ui.label(RichText::new("Open a folder of markdown notes from the sidebar to start.").weak());
        });
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
            match crate::screenshot::save(&image, path) {
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

/// The skin used until one is picked: the app's own, built into Patina.
const HOME_SKIN: &str = "Notebook";

/// A message for the status bar.
struct Status {
    level: Level,
    text: String,
}

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

fn load_skins() -> (Vec<Skin>, Vec<String>) {
    skin::load_all(skin::BUILT_IN, &skin::app_dir("notebook").into_iter().collect::<Vec<_>>())
}


impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some((id, path)) = self.picked.as_ref().and_then(|(id, rx)| Some((*id, rx.try_recv().ok()?))) {
            self.picked = None;
            self.picked_folder(id, path);
        }
        if let Some((id, files)) = self.picked_images.as_ref().and_then(|(id, rx)| Some((*id, rx.try_recv().ok()?))) {
            self.picked_images = None;
            self.picked_images(ctx, id, files);
        }
        self.dropped(ctx);
        if self.dialog.is_none() {
            // Before the editor sees Ctrl+PageUp/PageDown.
            if let Some(action) = tabs::keys(ctx, self.tabs.len(), self.shown) {
                self.tab_action(action);
            }
            self.shortcuts(ctx);
        }
        // Pick up changes made in other apps (a file manager, git, sync)
        // when the window comes back to the front.
        if ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::WindowFocused(true)))) {
            self.tab_mut().refresh();
        }
        // Autosave a moment after the last edit. Only the shown tab is
        // edited; the others were saved when they were left.
        let now = ctx.input(|i| i.time);
        let tab = self.tab();
        if tab.dirty()
            && let Some(note) = &tab.note
        {
            let wait = AUTOSAVE - (now - note.edited_at);
            if wait <= 0.0 {
                let saved = self.tab_mut().save();
                self.report(saved);
            } else {
                ctx.request_repaint_after(Duration::from_secs_f64(wait));
            }
        }
        if self.picked.is_some() || self.picked_images.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        self.remember_tabs();
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let custom = !self.config.system_title_bar;
        let style = self.skin().frame_style(ui.visuals());
        let radius = if custom { frame::corner_radius(&ctx, style.radius) } else { 0 };

        let header = frame::header_frame(ui.style(), &style, radius);
        egui::Panel::top("header").frame(header).show(ui, |ui| {
            // The bar's full height, padding and all, for the tabs to fill.
            let span = (ui.max_rect() + header.inner_margin).y_range();
            self.header(ui, &style, span)
        });

        // Patina's status bar (its ADR-0016). It spans the window, so it comes before the sidebar,
        // and holds both bottom corners.
        egui::Panel::bottom("status")
            .frame(
                egui::Frame::side_top_panel(ui.style())
                    .inner_margin(egui::Margin::symmetric(10, 4))
                    .corner_radius(egui::CornerRadius { sw: radius, se: radius, ..Default::default() }),
            )
            .show(ui, |ui| self.status_bar(ui));

        // Always there, so a tab without a folder can offer to open one.
        egui::Panel::left("sidebar")
            .frame(egui::Frame::side_top_panel(ui.style()))
            .default_size(220.0)
            .show(ui, |ui| self.sidebar(ui));

        let tab = &mut self.tabs[self.shown];
        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(ui.style()).inner_margin(0))
            .show(ui, |ui| {
                if tab.note.is_none() {
                    if tab.folder.is_some() {
                        ui.centered_and_justified(|ui| ui.label(RichText::new("No note open").weak()));
                    } else {
                        Self::welcome(ui);
                    }
                    return;
                }
                let now = ui.input(|i| i.time);
                egui::ScrollArea::vertical().id_salt(("note", tab.id)).auto_shrink(false).show(ui, |ui| {
                    // A readable line length, centred.
                    let width = ui.available_width().min(760.0);
                    let margin = ((ui.available_width() - width) / 2.0).max(24.0);
                    egui::Frame::NONE.inner_margin(egui::Margin::symmetric(margin as i8, 20)).show(ui, |ui| {
                        let changed = match &mut tab.source {
                            Some(source) => ui
                                .add(
                                    egui::TextEdit::multiline(source)
                                        .id_salt(("source", tab.id))
                                        .code_editor()
                                        .frame(egui::Frame::NONE)
                                        .desired_width(f32::INFINITY),
                                )
                                .changed(),
                            None => tab.editor.show(ui),
                        };
                        if changed {
                            if tab.source.is_some() {
                                // The source has no edit count of its own.
                                if let Some(note) = &mut tab.note {
                                    note.saved_edits = u64::MAX;
                                }
                            }
                            if let Some(note) = &mut tab.note {
                                note.edited_at = now;
                            }
                        }
                    });
                });
            });

        // After the editor has drawn: the sidebar click that opened the note
        // counts as a click outside the editor, which would drop a focus
        // requested any earlier this frame.
        if std::mem::take(&mut self.focus_editor) && self.tab().source.is_none() && self.dialog.is_none() {
            self.tab().editor.focus(&ctx);
        }
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
        ime::drop_repeated_preedits(ctx, raw_input);
    }

    fn on_exit(&mut self) {
        for tab in &mut self.tabs {
            let _ = tab.save();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn added_images_are_linked_inside_the_workspace_and_copied_from_outside() {
        let root = std::env::temp_dir().join(format!("notebook-images-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (work, outside) = (root.join("work"), root.join("outside"));
        std::fs::create_dir_all(work.join("Pics")).unwrap();
        std::fs::create_dir_all(work.join("Notes")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(work.join("Notes/a.md"), "# A\n").unwrap();
        std::fs::write(work.join("Pics/in.png"), "png").unwrap();
        std::fs::write(outside.join("cat photo.png"), "png").unwrap();
        // Taken, so the copy gets a new name.
        std::fs::write(work.join("Notes/cat photo.png"), "other").unwrap();

        let mut tab = Tab::new(0);
        tab.open_folder(work.clone()).unwrap();
        tab.open_note(work.join("Notes/a.md")).unwrap();
        tab.add_images(&[work.join("Pics/in.png"), outside.join("cat photo.png")], 0.0).unwrap();
        tab.save().unwrap();

        let md = std::fs::read_to_string(work.join("Notes/a.md")).unwrap();
        assert!(md.contains("![in](../Pics/in.png)"), "{md}");
        assert!(md.contains("![cat photo](<cat photo 2.png>)"), "{md}");
        assert_eq!(std::fs::read_to_string(work.join("Notes/cat photo 2.png")).unwrap(), "png");
        assert!(!work.join("Notes/in.png").exists(), "an image in the workspace isn't copied");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_home_skin_is_built_into_patina() {
        let (skins, errors) = skin::load_all(skin::BUILT_IN, &[]);
        assert!(errors.is_empty(), "{errors:?}");
        assert!(skins.iter().any(|s| s.name == HOME_SKIN));
    }
}
