//! The window: Patina's frame with File, Edit, View and Skin menus and the
//! documents as tabs in the title bar, Save on the right; the find bar
//! under it when open; the editor; and a status bar with the cursor's
//! place, the language, the indentation and the line ends.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use egui::{CornerRadius, FontId, Key, Modifiers, RichText, TextWrapMode, Ui, vec2};
use patina::frame::{self, FrameStyle, TitleBar};
use patina::skin::{self, Skin};
use patina::tabs::{self, TabAction, TabStrip};

use crate::config::{APP, Config};
use crate::document::{Document, OnDisk};
use crate::files::{self, Ending, Loaded};
use crate::find::{self, Query};
use crate::highlight;
use crate::indent;
use crate::session::{self, SavedTab, Session};
use crate::view::{self, Look};
use crate::watch::Watch;

/// How long after typing stops a tab's draft is written.
const DRAFT_DELAY: Duration = Duration::from_secs(1);
const SMALLEST_FONT: f32 = 8.0;
const LARGEST_FONT: f32 = 48.0;

/// Work done off the window's thread, come back.
enum Done {
    Loaded {
        id: u64,
        result: std::io::Result<Loaded>,
        again: bool,
    },
    Saved {
        id: u64,
        path: PathBuf,
        digest: u64,
        result: Result<(), String>,
        close: bool,
    },
    /// Files chosen to open.
    Picked(Vec<PathBuf>),
    /// Where to save a tab, or nothing if the dialog was cancelled.
    SaveTo {
        id: u64,
        path: Option<PathBuf>,
        close: bool,
    },
    DraftsWritten(Vec<(u64, Result<(), String>)>),
}

/// The find bar.
#[derive(Default)]
struct FindBar {
    open: bool,
    replacing: bool,
    query: Query,
    with: String,
    /// What was searched for, in which tab at which edit, so it's searched
    /// again only when something changes.
    searched: Option<(u64, u64, Query)>,
    error: Option<String>,
    focus: bool,
}

pub struct App {
    ctx: egui::Context,
    config: Config,
    skins: Vec<Skin>,
    skin: usize,
    skin_errors: Vec<String>,
    status: Option<Status>,
    docs: Vec<Document>,
    tab: usize,
    find: FindBar,
    /// The Go to Line field, while open.
    goto: Option<String>,
    focus_goto: bool,
    /// Give the editor the keyboard on the next frame.
    focus_editor: bool,
    /// A place (characters) in the tab in front to bring into view.
    scroll_to: Option<usize>,
    /// A tab with changes asking whether to save them before it closes.
    closing: Option<u64>,
    /// A file dialog is open.
    dialog: bool,
    send: Sender<Done>,
    done: Receiver<Done>,
    watch: Option<Watch>,
    /// Files from later starts of the app (ADR-0005).
    incoming: Option<Receiver<Vec<PathBuf>>>,
    title: String,
    session_changed: bool,
    screenshot: Option<(PathBuf, bool)>,
    /// Quitting: drafts and the session are written once.
    quitting: bool,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, files: Vec<PathBuf>) -> Self {
        patina::appearance::watch(&cc.egui_ctx);
        let ctx = cc.egui_ctx.clone();
        let config = Config::load();
        let (skins, skin_errors) = load_skins();
        let wanted = std::env::var("TEXT_EDITOR_SKIN").unwrap_or_else(|_| config.skin.clone());
        let skin = skins.iter().position(|s| s.name == wanted).unwrap_or(0);
        let (send, done) = channel();
        let screenshot = std::env::var_os("TEXT_EDITOR_SCREENSHOT").map(|p| (PathBuf::from(p), false));
        let mut app = App {
            incoming: if screenshot.is_some() { None } else { crate::instance::listen(&ctx) },
            ctx,
            config,
            skins,
            skin,
            skin_errors,
            status: None,
            docs: Vec::new(),
            tab: 0,
            find: FindBar::default(),
            goto: None,
            focus_goto: false,
            focus_editor: true,
            scroll_to: None,
            closing: None,
            dialog: false,
            send,
            done,
            watch: Watch::new(&cc.egui_ctx).inspect_err(|e| eprintln!("text-editor: can't watch files: {e}")).ok(),
            title: String::new(),
            session_changed: false,
            screenshot,
            quitting: false,
        };
        app.apply_skin(&cc.egui_ctx);
        if app.config.system_title_bar {
            cc.egui_ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(true));
        }
        app.restore();
        app.open(files);
        if app.docs.is_empty() {
            app.docs.push(Document::new(1, app.config.indent));
        }
        app.session_changed = true;
        app
    }

    // ---- Tabs and files ----

    /// The tabs open last time, their drafts read back.
    fn restore(&mut self) {
        let session = Session::load();
        session.sweep_drafts();
        for saved in &session.tabs {
            let draft = if saved.draft { session::read_draft(saved.id) } else { None };
            let mut doc = match &saved.path {
                Some(path) => Document::file(path.clone(), self.config.indent),
                None if draft.is_some() => Document::new(saved.untitled.max(1), self.config.indent),
                // An empty untitled tab isn't worth bringing back.
                None => continue,
            };
            doc.id = saved.id;
            doc.chosen_language = saved.language.clone();
            doc.place_cursor = Some(saved.cursor..saved.cursor);
            if let Some(text) = draft {
                doc.restored(text, self.config.indent);
                doc.edited();
                doc.draft_due = None;
            }
            if doc.loading {
                self.read(&doc, false);
            }
            self.docs.push(doc);
        }
        self.tab = session.tab.min(self.docs.len().saturating_sub(1));
    }

    fn session(&self) -> Session {
        let tabs = self
            .docs
            .iter()
            .filter(|d| !d.is_blank() || self.docs.len() == 1)
            .map(|d| SavedTab {
                id: d.id,
                path: d.path.clone(),
                untitled: d.untitled,
                draft: d.has_draft,
                language: d.chosen_language.clone(),
                cursor: view::selection(&self.ctx, d).map_or(0, |s| s.end),
            })
            .collect();
        Session { tabs, tab: self.tab }
    }

    fn current(&mut self) -> &mut Document {
        &mut self.docs[self.tab]
    }

    fn index_of(&self, id: u64) -> Option<usize> {
        self.docs.iter().position(|d| d.id == id)
    }

    /// Reads a tab's file on a thread; `again` for a change on disk.
    fn read(&self, doc: &Document, again: bool) {
        let Some(path) = doc.path.clone() else { return };
        let (id, send, ctx) = (doc.id, self.send.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let _ = send.send(Done::Loaded { id, result: files::read(&path), again });
            ctx.request_repaint();
        });
    }

    /// Opens files in tabs, or shows them where they're open already.
    fn open(&mut self, paths: Vec<PathBuf>) {
        for path in paths {
            let path = std::path::absolute(&path).unwrap_or(path);
            if path.is_dir() {
                self.status = Some(Status::error(format!("{} is a folder", path.display())));
                continue;
            }
            if let Some(i) = self.docs.iter().position(|d| d.path.as_ref() == Some(&path)) {
                self.tab = i;
                continue;
            }
            let doc = Document::file(path.clone(), self.config.indent);
            self.read(&doc, false);
            self.put_in_front(doc);
            self.config.opened(&path);
        }
        self.config.save();
        self.focus_editor = true;
        self.session_changed = true;
    }

    /// A new tab after the one in front, or in place of a blank one there.
    fn put_in_front(&mut self, doc: Document) {
        if self.docs.get(self.tab).is_some_and(Document::is_blank) {
            self.docs[self.tab] = doc;
        } else {
            self.tab = (self.tab + 1).min(self.docs.len());
            self.docs.insert(self.tab, doc);
        }
    }

    fn new_tab(&mut self) {
        let taken: Vec<usize> = self.docs.iter().filter(|d| d.path.is_none()).map(|d| d.untitled).collect();
        let number = (1..).find(|n| !taken.contains(n)).unwrap_or(1);
        self.tab = (self.tab + 1).min(self.docs.len());
        self.docs.insert(self.tab, Document::new(number, self.config.indent));
        self.focus_editor = true;
        self.session_changed = true;
    }

    /// Closes a tab, asking first if it has unsaved changes.
    fn close(&mut self, i: usize) {
        if self.docs[i].modified {
            self.tab = i;
            self.closing = Some(self.docs[i].id);
        } else {
            self.close_now(i);
        }
    }

    fn close_now(&mut self, i: usize) {
        let doc = self.docs.remove(i);
        session::remove_draft(doc.id);
        if self.docs.is_empty() {
            self.docs.push(Document::new(1, self.config.indent));
        }
        if i < self.tab || self.tab >= self.docs.len() {
            self.tab = self.tab.saturating_sub(1);
        }
        self.focus_editor = true;
        self.session_changed = true;
    }

    fn tab_action(&mut self, action: TabAction) {
        match action {
            TabAction::New => self.new_tab(),
            TabAction::Show(i) => {
                self.tab = i.min(self.docs.len() - 1);
                self.focus_editor = true;
                self.session_changed = true;
            }
            TabAction::Close(i) => self.close(i),
        }
    }

    fn pick_files(&mut self) {
        if self.dialog {
            return;
        }
        self.dialog = true;
        let folder = self.docs[self.tab].path.as_ref().and_then(|p| p.parent()).map(Path::to_path_buf);
        let (send, ctx) = (self.send.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let mut dialog = rfd::FileDialog::new().set_title("Open");
            if let Some(folder) = folder {
                dialog = dialog.set_directory(folder);
            }
            let _ = send.send(Done::Picked(dialog.pick_files().unwrap_or_default()));
            ctx.request_repaint();
        });
    }

    /// Saves the tab, asking where if it has no file; closes it after if
    /// `close`.
    fn save(&mut self, i: usize, close: bool) {
        let doc = &self.docs[i];
        if let Some(why) = &doc.read_only {
            self.status = Some(Status::error(format!("Can't save {}: {why}", doc.name())));
            return;
        }
        match doc.path.clone() {
            Some(path) => self.write(i, path, close),
            None => self.save_as(i, close),
        }
    }

    fn save_as(&mut self, i: usize, close: bool) {
        if self.dialog {
            return;
        }
        self.dialog = true;
        let doc = &self.docs[i];
        let (id, name) = (doc.id, doc.path.as_ref().map_or_else(|| format!("{}.txt", doc.name()), |_| doc.name()));
        let folder = doc.path.as_ref().and_then(|p| p.parent()).map(Path::to_path_buf).or_else(dirs::document_dir);
        let (send, ctx) = (self.send.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let mut dialog = rfd::FileDialog::new().set_title("Save As").set_file_name(name);
            if let Some(folder) = folder {
                dialog = dialog.set_directory(folder);
            }
            let _ = send.send(Done::SaveTo { id, path: dialog.save_file(), close });
            ctx.request_repaint();
        });
    }

    /// Writes a tab's text to `path` on a thread.
    fn write(&mut self, i: usize, path: PathBuf, close: bool) {
        let doc = &self.docs[i];
        let bytes = files::encode(&doc.text, doc.ending, doc.bom);
        let (id, digest) = (doc.id, files::digest(&doc.text));
        let (send, ctx) = (self.send.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let result = files::write(&path, &bytes).map_err(|e| e.to_string());
            let _ = send.send(Done::Saved { id, path, digest, result, close });
            ctx.request_repaint();
        });
    }

    fn save_all(&mut self) {
        for i in 0..self.docs.len() {
            if self.docs[i].modified && self.docs[i].path.is_some() && self.docs[i].read_only.is_none() {
                self.save(i, false);
            }
        }
    }

    /// Throws away a tab's changes, reading its file again.
    fn discard_changes(&mut self) {
        let doc = &mut self.docs[self.tab];
        if doc.path.is_none() {
            return;
        }
        view::checkpoint(&self.ctx, doc);
        doc.loading = true;
        doc.has_draft = false;
        session::remove_draft(doc.id);
        let doc = &self.docs[self.tab];
        self.read(doc, false);
    }

    /// Takes in what came back from threads.
    fn take_done(&mut self) {
        while let Ok(done) = self.done.try_recv() {
            match done {
                Done::Loaded { id, result, again } => self.loaded(id, result, again),
                Done::Saved { id, path, digest, result, close } => {
                    let Some(i) = self.index_of(id) else { continue };
                    match result {
                        Ok(()) => {
                            let doc = &mut self.docs[i];
                            doc.saved_as(path.clone(), digest);
                            if !doc.modified {
                                doc.has_draft = false;
                                doc.draft_due = None;
                                session::remove_draft(id);
                            }
                            self.status = Some(Status::info(format!("Saved {}", path.display())));
                            self.config.opened(&path);
                            self.config.save();
                            self.session_changed = true;
                            if close {
                                self.close_now(i);
                            }
                        }
                        Err(e) => {
                            self.status = Some(Status::error(format!("Can't save {}: {e}", path.display())));
                        }
                    }
                }
                Done::Picked(paths) => {
                    self.dialog = false;
                    self.open(paths);
                }
                Done::SaveTo { id, path, close } => {
                    self.dialog = false;
                    if let (Some(i), Some(path)) = (self.index_of(id), path) {
                        self.write(i, path, close);
                    }
                }
                Done::DraftsWritten(results) => {
                    for (id, result) in results {
                        if let Err(e) = result {
                            self.status = Some(Status::error(format!("Can't keep a draft: {e}")));
                        } else if let Some(i) = self.index_of(id) {
                            self.docs[i].has_draft = true;
                        }
                    }
                    self.session_changed = true;
                }
            }
        }
    }

    fn loaded(&mut self, id: u64, result: std::io::Result<Loaded>, again: bool) {
        let Some(i) = self.index_of(id) else { return };
        let default = self.config.indent;
        let doc = &mut self.docs[i];
        match (result, again) {
            (Ok(loaded), false) => {
                doc.loaded(loaded, default);
                view::forget_undo(&self.ctx, doc);
            }
            (Err(e), false) => {
                doc.load_failed();
                if e.kind() == std::io::ErrorKind::NotFound {
                    self.status = Some(Status::info(format!("{} is a new file", doc.name())));
                } else {
                    doc.read_only = Some(format!("It can't be read: {e}."));
                    self.status = Some(Status::error(format!("Can't read {}: {e}", doc.name())));
                }
            }
            // A save of ours, or a change with nothing here to lose.
            (Ok(loaded), true) if doc.is_saved(&loaded.text) => {}
            (Ok(loaded), true) if !doc.modified => {
                view::checkpoint(&self.ctx, doc);
                let cursor = view::selection(&self.ctx, doc);
                doc.loaded(loaded, default);
                doc.place_cursor = cursor;
                self.status = Some(Status::info(format!("{} changed on disk, and was read again", doc.name())));
            }
            (Ok(_), true) => doc.on_disk = Some(OnDisk::Changed),
            (Err(e), true) if e.kind() == std::io::ErrorKind::NotFound => {
                if doc.on_disk != Some(OnDisk::Deleted) {
                    doc.deleted();
                }
            }
            (Err(_), true) => {}
        }
    }

    /// Writes the drafts of tabs edited a moment ago, on a thread, and
    /// removes those of tabs back as saved.
    fn keep_drafts(&mut self, now: bool) {
        let mut due = Vec::new();
        for doc in &mut self.docs {
            let Some(at) = doc.draft_due else { continue };
            if !now && at.elapsed() < DRAFT_DELAY {
                self.ctx.request_repaint_after(DRAFT_DELAY - at.elapsed());
                continue;
            }
            doc.draft_due = None;
            if doc.modified {
                due.push((doc.id, doc.text.clone()));
            } else if doc.has_draft {
                doc.has_draft = false;
                session::remove_draft(doc.id);
                self.session_changed = true;
            }
        }
        if due.is_empty() {
            return;
        }
        let write = move || due.iter().map(|(id, text)| (*id, session::write_draft(*id, text))).collect::<Vec<_>>();
        if now {
            let results = write();
            for (id, result) in results {
                match (result, self.index_of(id)) {
                    (Ok(()), Some(i)) => self.docs[i].has_draft = true,
                    (Err(e), _) => eprintln!("text-editor: can't keep a draft: {e}"),
                    _ => {}
                }
            }
        } else {
            let (send, ctx) = (self.send.clone(), self.ctx.clone());
            std::thread::spawn(move || {
                let _ = send.send(Done::DraftsWritten(write()));
                ctx.request_repaint();
            });
        }
    }

    /// Writes everything that has to outlive the window.
    fn quit(&mut self) {
        if std::mem::replace(&mut self.quitting, true) {
            return;
        }
        if self.screenshot.is_some() {
            return;
        }
        self.keep_drafts(true);
        self.session().save();
        self.config.save();
        crate::instance::release();
    }

    /// Reads tabs whose files changed on disk.
    fn watch_files(&mut self) {
        let Some(watch) = &mut self.watch else { return };
        watch.set(self.docs.iter().filter_map(|d| d.path.as_deref()));
        let Some(changes) = watch.changes(&self.ctx) else { return };
        for doc in &self.docs {
            if !doc.loading && doc.path.as_deref().is_some_and(|p| changes.touches(p)) {
                self.read(doc, true);
            }
        }
    }

    // ---- Find, replace and go to line ----

    fn open_find(&mut self, replacing: bool) {
        // The selection, if it's on one line, is what to find.
        let doc = &self.docs[self.tab];
        if let Some(sel) = view::selection(&self.ctx, doc).filter(|s| !s.is_empty()) {
            let (a, b) = (indent::byte_at(&doc.text, sel.start), indent::byte_at(&doc.text, sel.end));
            if !doc.text[a..b].contains('\n') {
                self.find.query.text = doc.text[a..b].to_string();
            }
        }
        self.find.open = true;
        self.find.replacing = replacing;
        self.find.focus = true;
        self.goto = None;
    }

    fn close_find(&mut self) {
        self.find.open = false;
        self.find.searched = None;
        let doc = self.current();
        (doc.found, doc.current) = (Vec::new(), None);
        doc.galley = None;
        self.focus_editor = true;
    }

    /// Searches the tab in front again if the search or its text changed;
    /// a new search goes to the first match from the cursor.
    fn search(&mut self) {
        if !self.find.open {
            return;
        }
        let doc = &self.docs[self.tab];
        let key = (doc.id, doc.version, self.find.query.clone());
        if self.find.searched.as_ref() == Some(&key) {
            return;
        }
        let new_query = self.find.searched.as_ref().is_none_or(|(id, _, q)| *id != doc.id || *q != self.find.query);
        self.find.searched = Some(key);
        let found = self.find.query.find_all(&doc.text);
        let selection = view::selection(&self.ctx, doc).unwrap_or(0..0);
        let doc = &mut self.docs[self.tab];
        match found {
            Ok(found) => {
                self.find.error = None;
                let sel = indent::byte_at(&doc.text, selection.start)..indent::byte_at(&doc.text, selection.end);
                doc.current = found.iter().position(|m| *m == sel);
                doc.found = found;
                if new_query && doc.current.is_none() && !doc.found.is_empty() {
                    self.step(true, Some(sel.start));
                }
            }
            Err(e) => {
                self.find.error = Some(e);
                (doc.found, doc.current) = (Vec::new(), None);
            }
        }
    }

    /// Selects the next match (or the one before), from the selection or
    /// from byte `from`.
    fn step(&mut self, forward: bool, from: Option<usize>) {
        let ctx = self.ctx.clone();
        let doc = &mut self.docs[self.tab];
        let from = from.unwrap_or_else(|| {
            let sel = view::selection(&ctx, doc).unwrap_or(0..0);
            indent::byte_at(&doc.text, if forward { sel.end } else { sel.start })
        });
        let Some(i) = find::next(&doc.found, from, forward) else { return };
        doc.current = Some(i);
        let m = doc.found[i].clone();
        let (a, b) = (indent::char_at(&doc.text, m.start), indent::char_at(&doc.text, m.end));
        view::select(&ctx, doc, a..b);
        self.scroll_to = Some(a);
    }

    /// Replaces the current match, then goes to the next.
    fn replace_one(&mut self) {
        self.search();
        let ctx = self.ctx.clone();
        let doc = &mut self.docs[self.tab];
        let sel = view::selection(&ctx, doc).unwrap_or(0..0);
        let sel = indent::byte_at(&doc.text, sel.start)..indent::byte_at(&doc.text, sel.end);
        let on_match = doc.current.and_then(|i| doc.found.get(i)).is_some_and(|m| *m == sel);
        if !on_match || doc.read_only.is_some() {
            self.step(true, None);
            return;
        }
        let Some(with) = self.find.query.replacement(&doc.text, &sel, &self.find.with) else { return };
        view::checkpoint(&ctx, doc);
        doc.text.replace_range(sel.clone(), &with);
        doc.edited();
        let after = sel.start + with.len();
        let at = indent::char_at(&doc.text, after);
        view::select(&ctx, doc, at..at);
        self.search();
        self.step(true, Some(after));
    }

    fn replace_all(&mut self) {
        let ctx = self.ctx.clone();
        let doc = &mut self.docs[self.tab];
        if doc.read_only.is_some() {
            return;
        }
        match self.find.query.replace_all(&doc.text, &self.find.with) {
            Ok((_, 0)) => self.status = Some(Status::info("Nothing to replace")),
            Ok((text, n)) => {
                view::checkpoint(&ctx, doc);
                doc.text = text;
                doc.edited();
                self.status = Some(Status::info(format!("Replaced {n} {}", if n == 1 { "match" } else { "matches" })));
            }
            Err(e) => self.find.error = Some(e),
        }
    }

    /// Goes to "LINE" or "LINE:COLUMN", from 1.
    fn go_to_line(&mut self, place: &str) {
        let mut parts = place.trim().splitn(2, [':', ',']);
        let Some(line) = parts.next().and_then(|l| l.trim().parse::<usize>().ok()) else { return };
        let column = parts.next().and_then(|c| c.trim().parse::<usize>().ok()).unwrap_or(1);
        let doc = &self.docs[self.tab];
        let start = if line <= 1 {
            0
        } else {
            doc.text.match_indices('\n').nth(line - 2).map_or(doc.text.len(), |(i, _)| i + 1)
        };
        let end = doc.text[start..].find('\n').map_or(doc.text.len(), |i| start + i);
        let at =
            indent::char_at(&doc.text, start) + (column.saturating_sub(1)).min(doc.text[start..end].chars().count());
        view::select(&self.ctx, doc, at..at);
        self.scroll_to = Some(at);
        self.focus_editor = true;
    }

    // ---- Keys ----

    fn keys(&mut self, ctx: &egui::Context) {
        if self.closing.is_some() {
            return;
        }
        let shift = Modifiers::COMMAND | Modifiers::SHIFT;
        let (mut save_as, mut save, mut open, mut new, mut find, mut replace) = Default::default();
        let (mut next, mut previous, mut goto, mut quit, mut zoom, mut escape) = Default::default();
        ctx.input_mut(|i| {
            save_as = i.consume_key(shift, Key::S);
            save = i.consume_key(Modifiers::COMMAND, Key::S);
            open = i.consume_key(Modifiers::COMMAND, Key::O);
            new = i.consume_key(Modifiers::COMMAND, Key::N);
            find = i.consume_key(Modifiers::COMMAND, Key::F);
            replace = i.consume_key(Modifiers::COMMAND, Key::H);
            previous = i.consume_key(shift, Key::G) || i.consume_key(Modifiers::SHIFT, Key::F3);
            next = i.consume_key(Modifiers::COMMAND, Key::G) || i.consume_key(Modifiers::NONE, Key::F3);
            goto = i.consume_key(Modifiers::COMMAND, Key::I);
            quit = i.consume_key(Modifiers::COMMAND, Key::Q);
            zoom = if i.consume_key(Modifiers::COMMAND, Key::Plus) || i.consume_key(Modifiers::COMMAND, Key::Equals) {
                1.0
            } else if i.consume_key(Modifiers::COMMAND, Key::Minus) {
                -1.0
            } else if i.consume_key(Modifiers::COMMAND, Key::Num0) {
                f32::NAN
            } else {
                0.0
            };
            escape = (self.find.open || self.goto.is_some()) && i.consume_key(Modifiers::NONE, Key::Escape);
        });
        if save_as {
            self.save_as(self.tab, false);
        } else if save {
            self.save(self.tab, false);
        }
        if open {
            self.pick_files();
        }
        if new {
            self.new_tab();
        }
        if find || replace {
            self.open_find(replace);
        }
        if next || previous {
            if !self.find.open && !self.find.query.text.is_empty() {
                self.find.open = true;
            }
            self.search();
            self.step(next, None);
        }
        if goto {
            self.goto = Some(String::new());
            self.focus_goto = true;
            self.find.open = false;
        }
        if escape {
            self.goto = None;
            self.close_find();
        }
        if zoom != 0.0 {
            self.zoom(zoom);
        }
        if quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if let Some(action) = tabs::keys(ctx, self.docs.len(), self.tab) {
            self.tab_action(action);
        }
    }

    /// Bigger or smaller text; NaN for the usual size.
    fn zoom(&mut self, by: f32) {
        let size = if by.is_nan() { Config::default().font_size } else { self.config.font_size + by };
        self.config.font_size = size.clamp(SMALLEST_FONT, LARGEST_FONT);
        self.config.save();
    }

    // ---- Drawing ----

    fn header(&mut self, ui: &mut Ui, style: &FrameStyle, span: egui::Rangef) {
        let titles: Vec<String> = self.docs.iter().map(Document::title).collect();
        let mut action = None;
        let custom = !self.config.system_title_bar;
        let shown = self.tab;
        // The bar's two sides each need the window: one at a time.
        let this = std::cell::RefCell::new(&mut *self);
        TitleBar::new("", style).custom(custom).menu_bar(true).span(span).show(
            ui,
            |ui| {
                ui.menu_button("File", |ui| this.borrow_mut().file_menu(ui));
                ui.menu_button("Edit", |ui| this.borrow_mut().edit_menu(ui));
                ui.menu_button("View", |ui| this.borrow_mut().view_menu(ui));
                ui.menu_button("Skin", |ui| this.borrow_mut().skin_menu(ui));
                action = TabStrip::new(&titles, shown, style).span(span).show(ui);
            },
            |ui| {
                let mut app = this.borrow_mut();
                let doc = &app.docs[app.tab];
                let can = (doc.modified || doc.path.is_none()) && doc.read_only.is_none() && !doc.loading;
                let tip = if doc.path.is_some() { "Save (Ctrl+S)" } else { "Save as a file (Ctrl+S)" };
                if ui.add_enabled(can, egui::Button::new("Save").min_size(vec2(0.0, 24.0))).on_hover_text(tip).clicked()
                {
                    let tab = app.tab;
                    app.save(tab, false);
                }
            },
        );
        if let Some(action) = action {
            self.tab_action(action);
        }
    }

    fn file_menu(&mut self, ui: &mut Ui) {
        if ui.add(egui::Button::new("New").shortcut_text(weak(ui, "Ctrl+N"))).clicked() {
            self.new_tab();
        }
        if ui.add(egui::Button::new("Open…").shortcut_text(weak(ui, "Ctrl+O"))).clicked() {
            self.pick_files();
        }
        ui.add_enabled_ui(!self.config.recent.is_empty(), |ui| {
            ui.menu_button("Open Recent", |ui| {
                ui.style_mut().wrap_mode = Some(TextWrapMode::Extend);
                for path in self.config.recent.clone() {
                    let name =
                        path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into());
                    if ui.button(name).on_hover_text(path.display().to_string()).clicked() {
                        self.open(vec![path]);
                    }
                }
                ui.separator();
                if ui.button("Clear List").clicked() {
                    self.config.recent.clear();
                    self.config.save();
                }
            });
        });
        ui.separator();
        let doc = &self.docs[self.tab];
        let (has_path, modified, writable) = (doc.path.is_some(), doc.modified, doc.read_only.is_none());
        if ui.add_enabled(writable, egui::Button::new("Save").shortcut_text(weak(ui, "Ctrl+S"))).clicked() {
            self.save(self.tab, false);
        }
        if ui.add(egui::Button::new("Save As…").shortcut_text(weak(ui, "Ctrl+Shift+S"))).clicked() {
            self.save_as(self.tab, false);
        }
        if ui
            .add_enabled(self.docs.iter().any(|d| d.modified && d.path.is_some()), egui::Button::new("Save All"))
            .clicked()
        {
            self.save_all();
        }
        if ui
            .add_enabled(has_path && modified, egui::Button::new("Discard Changes"))
            .on_hover_text("Read the file again, as it was last saved")
            .clicked()
        {
            self.discard_changes();
        }
        ui.separator();
        let path = self.docs[self.tab].path.clone();
        if ui.add_enabled(path.is_some(), egui::Button::new("Show in Files")).clicked()
            && let Some(folder) = path.as_ref().and_then(|p| p.parent())
            && let Err(e) = patina::open::path(folder)
        {
            self.status = Some(Status::error(format!("Can't open {}: {e}", folder.display())));
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
        if ui.add(egui::Button::new("Close Tab").shortcut_text(weak(ui, "Ctrl+W"))).clicked() {
            self.close(self.tab);
        }
        if ui.add(egui::Button::new("Quit").shortcut_text(weak(ui, "Ctrl+Q"))).clicked() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn edit_menu(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let (can_undo, can_redo) = view::can_undo(&ctx, &self.docs[self.tab]);
        let writable = self.docs[self.tab].read_only.is_none();
        if ui.add_enabled(can_undo && writable, egui::Button::new("Undo").shortcut_text(weak(ui, "Ctrl+Z"))).clicked() {
            view::undo(&ctx, self.current(), false);
            self.focus_editor = true;
        }
        if ui
            .add_enabled(can_redo && writable, egui::Button::new("Redo").shortcut_text(weak(ui, "Ctrl+Shift+Z")))
            .clicked()
        {
            view::undo(&ctx, self.current(), true);
            self.focus_editor = true;
        }
        ui.separator();
        let mut command = None;
        if ui.add_enabled(writable, egui::Button::new("Cut").shortcut_text(weak(ui, "Ctrl+X"))).clicked() {
            command = Some(egui::ViewportCommand::RequestCut);
        }
        if ui.add(egui::Button::new("Copy").shortcut_text(weak(ui, "Ctrl+C"))).clicked() {
            command = Some(egui::ViewportCommand::RequestCopy);
        }
        if ui.add_enabled(writable, egui::Button::new("Paste").shortcut_text(weak(ui, "Ctrl+V"))).clicked() {
            command = Some(egui::ViewportCommand::RequestPaste);
        }
        if ui.add(egui::Button::new("Select All").shortcut_text(weak(ui, "Ctrl+A"))).clicked() {
            let doc = &self.docs[self.tab];
            view::select(&ctx, doc, 0..doc.text.chars().count());
            self.focus_editor = true;
        }
        if let Some(command) = command {
            ctx.send_viewport_cmd(command);
            ctx.memory_mut(|m| m.request_focus(view::text_id(&self.docs[self.tab])));
        }
        ui.separator();
        if ui.add(egui::Button::new("Find…").shortcut_text(weak(ui, "Ctrl+F"))).clicked() {
            self.open_find(false);
        }
        if ui.add(egui::Button::new("Find and Replace…").shortcut_text(weak(ui, "Ctrl+H"))).clicked() {
            self.open_find(true);
        }
        let has_query = !self.find.query.text.is_empty();
        if ui.add_enabled(has_query, egui::Button::new("Find Next").shortcut_text(weak(ui, "Ctrl+G"))).clicked() {
            self.find.open = true;
            self.search();
            self.step(true, None);
        }
        if ui
            .add_enabled(has_query, egui::Button::new("Find Previous").shortcut_text(weak(ui, "Ctrl+Shift+G")))
            .clicked()
        {
            self.find.open = true;
            self.search();
            self.step(false, None);
        }
        if ui.add(egui::Button::new("Go to Line…").shortcut_text(weak(ui, "Ctrl+I"))).clicked() {
            self.goto = Some(String::new());
            self.focus_goto = true;
            self.find.open = false;
        }
    }

    fn view_menu(&mut self, ui: &mut Ui) {
        let mut changed = false;
        changed |= ui.checkbox(&mut self.config.line_numbers, "Line Numbers").changed();
        changed |= ui.checkbox(&mut self.config.wrap, "Wrap Lines").changed();
        changed |= ui.checkbox(&mut self.config.highlight_line, "Highlight Current Line").changed();
        if changed {
            self.config.save();
        }
        ui.separator();
        if ui.add(egui::Button::new("Zoom In").shortcut_text(weak(ui, "Ctrl++"))).clicked() {
            self.zoom(1.0);
        }
        if ui.add(egui::Button::new("Zoom Out").shortcut_text(weak(ui, "Ctrl+-"))).clicked() {
            self.zoom(-1.0);
        }
        if ui.add(egui::Button::new("Actual Size").shortcut_text(weak(ui, "Ctrl+0"))).clicked() {
            self.zoom(f32::NAN);
        }
        ui.separator();
        ui.menu_button("Language", |ui| self.language_menu(ui));
        ui.menu_button("Indentation", |ui| self.indent_menu(ui));
        ui.menu_button("Line Ends", |ui| self.ending_menu(ui));
    }

    fn language_menu(&mut self, ui: &mut Ui) {
        ui.style_mut().wrap_mode = Some(TextWrapMode::Extend);
        let doc = &mut self.docs[self.tab];
        let language = doc.language();
        let mut chosen = None;
        egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
            if ui.radio(language.as_deref().is_none_or(|l| l == "Plain Text"), "Plain Text").clicked() {
                chosen = Some(None);
            }
            for name in highlight::languages() {
                if ui.radio(language.as_deref() == Some(name), name).clicked() {
                    chosen = Some(Some(name.to_string()));
                }
            }
        });
        if let Some(chosen) = chosen {
            // "Plain Text" chosen by hand is kept as a choice too.
            doc.chosen_language = Some(chosen.unwrap_or_else(|| "Plain Text".into()));
            self.session_changed = true;
            ui.close();
        }
    }

    fn indent_menu(&mut self, ui: &mut Ui) {
        let doc = &mut self.docs[self.tab];
        let mut indent = doc.indent;
        ui.radio_value(&mut indent.spaces, true, "Spaces");
        ui.radio_value(&mut indent.spaces, false, "Tabs");
        ui.separator();
        for width in [2, 4, 8] {
            ui.radio_value(&mut indent.width, width, format!("Width {width}"));
        }
        if indent != doc.indent {
            doc.indent = indent;
            doc.galley = None;
            // New files indent the same way.
            self.config.indent = indent;
            self.config.save();
        }
    }

    fn ending_menu(&mut self, ui: &mut Ui) {
        let doc = &mut self.docs[self.tab];
        let mut ending = doc.ending;
        ui.radio_value(&mut ending, Ending::Lf, "Unix and macOS (LF)");
        ui.radio_value(&mut ending, Ending::CrLf, "Windows (CRLF)");
        doc.set_ending(ending);
    }

    /// Rescans the built-in and user skins, keeping the one in use by name.
    fn reload_skins(&mut self) {
        let name = self.skins[self.skin].name.clone();
        (self.skins, self.skin_errors) = load_skins();
        self.skin = self.skins.iter().position(|s| s.name == name).unwrap_or(0);
    }

    fn apply_skin(&self, ctx: &egui::Context) {
        let skin = &self.skins[self.skin];
        skin.apply(ctx);
        if skin.follows_desktop() {
            ctx.set_theme(self.config.theme());
        }
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

    /// The find bar: the search and its options, how many matches, and
    /// replacing.
    fn find_bar(&mut self, ui: &mut Ui) {
        let mut query = self.find.query.clone();
        let (mut next, mut previous) = (false, false);
        ui.horizontal(|ui| {
            let field = ui.add(
                egui::TextEdit::singleline(&mut query.text)
                    .id_salt("find")
                    .hint_text("Find")
                    .desired_width(260.0)
                    .min_size(vec2(0.0, 24.0)),
            );
            if std::mem::take(&mut self.find.focus) {
                field.request_focus();
                select_all(ui, &field, &query.text);
            }
            if field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                (next, previous) = if ui.input(|i| i.modifiers.shift) { (false, true) } else { (true, false) };
                field.request_focus();
            }
            toggle(ui, &mut query.case, "Aa", "Match case");
            toggle(ui, &mut query.whole_word, "W", "Whole words");
            toggle(ui, &mut query.regex, ".*", "Regular expression");
            if ui
                .add(egui::Button::new("⏶").min_size(vec2(28.0, 24.0)))
                .on_hover_text("Previous (Ctrl+Shift+G)")
                .clicked()
            {
                previous = true;
            }
            if ui.add(egui::Button::new("⏷").min_size(vec2(28.0, 24.0))).on_hover_text("Next (Ctrl+G)").clicked() {
                next = true;
            }
            let doc = &self.docs[self.tab];
            let count = match (&self.find.error, doc.current) {
                (Some(e), _) => RichText::new(e).color(ui.visuals().error_fg_color),
                _ if query.text.is_empty() => RichText::new(""),
                _ if doc.found.is_empty() => RichText::new("No matches").color(ui.visuals().weak_text_color()),
                (None, Some(i)) => {
                    RichText::new(format!("{} of {}", i + 1, doc.found.len())).color(ui.visuals().weak_text_color())
                }
                (None, None) => {
                    RichText::new(format!("{} matches", doc.found.len())).color(ui.visuals().weak_text_color())
                }
            };
            ui.label(count.small());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(egui::Button::new("Close").min_size(vec2(0.0, 24.0))).on_hover_text("Escape").clicked() {
                    self.close_find();
                }
                let label = if self.find.replacing { "Hide Replace" } else { "Replace…" };
                if ui.add(egui::Button::new(label).min_size(vec2(0.0, 24.0))).clicked() {
                    self.find.replacing = !self.find.replacing;
                }
            });
        });
        let (mut one, mut all) = (false, false);
        if self.find.replacing {
            ui.horizontal(|ui| {
                let field = ui.add(
                    egui::TextEdit::singleline(&mut self.find.with)
                        .id_salt("replace")
                        .hint_text("Replace with")
                        .desired_width(260.0)
                        .min_size(vec2(0.0, 24.0)),
                );
                if field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    one = true;
                    field.request_focus();
                }
                let writable = self.docs[self.tab].read_only.is_none();
                one |= ui.add_enabled(writable, egui::Button::new("Replace").min_size(vec2(0.0, 24.0))).clicked();
                all |= ui.add_enabled(writable, egui::Button::new("Replace All").min_size(vec2(0.0, 24.0))).clicked();
            });
        }
        if query != self.find.query {
            self.find.query = query;
        }
        if !self.find.open {
            return;
        }
        self.search();
        if next || previous {
            self.step(next, None);
        }
        if one {
            self.replace_one();
        }
        if all {
            self.replace_all();
        }
    }

    fn goto_bar(&mut self, ui: &mut Ui) {
        let Some(mut place) = self.goto.take() else { return };
        let mut keep = true;
        ui.horizontal(|ui| {
            ui.label("Go to line");
            let field = ui.add(
                egui::TextEdit::singleline(&mut place)
                    .hint_text("Line, or line:column")
                    .desired_width(160.0)
                    .min_size(vec2(0.0, 24.0)),
            );
            if std::mem::take(&mut self.focus_goto) {
                field.request_focus();
            }
            if field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                self.go_to_line(&place);
                keep = false;
            }
            let lines = self.docs[self.tab].text.lines().count().max(1);
            ui.label(RichText::new(format!("of {lines}")).small().color(ui.visuals().weak_text_color()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(egui::Button::new("Close").min_size(vec2(0.0, 24.0))).clicked() {
                    keep = false;
                    self.focus_editor = true;
                }
            });
        });
        if keep {
            self.goto = Some(place);
        }
    }

    /// The version and latest message on the left; the cursor's place,
    /// language, indentation and line ends on the right.
    fn status_bar(&mut self, ui: &mut Ui, selection: Option<std::ops::Range<usize>>) {
        ui.horizontal(|ui| {
            // Just the version: the app's name is in the window list.
            ui.label(weak(ui, &format!("v{}", env!("CARGO_PKG_VERSION"))).small());
            let skin_errors = (!self.skin_errors.is_empty()).then(|| {
                let n = self.skin_errors.len();
                (Status::error(format!("Couldn't load {n} skin{}", if n == 1 { "" } else { "s" })), true)
            });
            if let Some((status, list)) = skin_errors.or(self.status.clone().map(|s| (s, false))) {
                ui.separator();
                let color = match status.level {
                    Level::Error => ui.visuals().error_fg_color,
                    Level::Info => ui.visuals().weak_text_color(),
                };
                let resp = ui.add(egui::Label::new(RichText::new(status.text).color(color).small()).truncate());
                if list {
                    resp.on_hover_text(self.skin_errors.join("\n"));
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let doc = &self.docs[self.tab];
                let ending = doc.ending.label();
                let indent = doc.indent.label();
                let language = doc.language().unwrap_or_else(|| "Plain Text".into());
                ui.menu_button(RichText::new(ending).small(), |ui| self.ending_menu(ui));
                ui.menu_button(RichText::new(indent).small(), |ui| self.indent_menu(ui));
                ui.menu_button(RichText::new(language).small(), |ui| self.language_menu(ui));
                let doc = &self.docs[self.tab];
                if let Some(sel) = selection {
                    let at = indent::byte_at(&doc.text, sel.end);
                    let line = doc.text[..at].bytes().filter(|&b| b == b'\n').count() + 1;
                    let line_start = doc.text[..at].rfind('\n').map_or(0, |i| i + 1);
                    let column = doc.text[line_start..at].chars().count() + 1;
                    let mut place = format!("Ln {line}, Col {column}");
                    if !sel.is_empty() {
                        place.push_str(&format!(" ({} selected)", sel.len()));
                    }
                    ui.label(weak(ui, &place).small());
                }
            });
        });
    }

    /// A strip above the text for what the tab needs said: it can't be
    /// edited, or its file changed or went on disk.
    fn notice(&mut self, ui: &mut Ui) {
        let doc = &self.docs[self.tab];
        let (text, actions): (String, &[&str]) = match (doc.on_disk, &doc.read_only) {
            (Some(OnDisk::Changed), _) => {
                (format!("{} changed on disk, and has changes here too.", doc.name()), &["Read Again", "Keep Mine"])
            }
            (Some(OnDisk::Deleted), _) => (format!("{} was deleted or moved.", doc.name()), &["Save", "Close"]),
            (None, Some(why)) => (why.clone(), &[]),
            (None, None) => return,
        };
        let mut clicked = None;
        egui::Frame::new().fill(ui.visuals().faint_bg_color).inner_margin(egui::Margin::symmetric(12, 6)).show(
            ui,
            |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(text).color(ui.visuals().warn_fg_color));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        for action in actions.iter().rev() {
                            if ui.add(egui::Button::new(*action).min_size(vec2(0.0, 24.0))).clicked() {
                                clicked = Some(*action);
                            }
                        }
                    });
                });
            },
        );
        match clicked {
            Some("Read Again") => {
                self.current().on_disk = None;
                self.discard_changes();
            }
            Some("Keep Mine") => self.current().on_disk = None,
            Some("Save") => self.save(self.tab, false),
            Some("Close") => self.close_now(self.tab),
            _ => {}
        }
    }

    /// Asks whether to save a tab's changes before it closes.
    fn closing_dialog(&mut self, ctx: &egui::Context) {
        let Some(id) = self.closing else { return };
        let Some(i) = self.index_of(id) else {
            self.closing = None;
            return;
        };
        let name = self.docs[i].name();
        let untitled = self.docs[i].path.is_none();
        let mut choice = None;
        let modal = egui::Modal::new(egui::Id::new("closing")).show(ctx, |ui| {
            ui.set_width(360.0);
            ui.heading(format!("Save changes to “{name}”?"));
            ui.add_space(6.0);
            ui.label("If you don't, they're lost.");
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.add(egui::Button::new("Close Without Saving").min_size(vec2(0.0, 24.0))).clicked() {
                    choice = Some("discard");
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let save = if untitled { "Save As…" } else { "Save" };
                    if ui.add(egui::Button::new(save).min_size(vec2(0.0, 24.0))).clicked()
                        || ui.input(|i| i.key_pressed(Key::Enter))
                    {
                        choice = Some("save");
                    }
                    if ui.add(egui::Button::new("Cancel").min_size(vec2(0.0, 24.0))).clicked() {
                        choice = Some("cancel");
                    }
                });
            });
        });
        if modal.should_close() {
            choice = choice.or(Some("cancel"));
        }
        match choice {
            Some("discard") => {
                self.closing = None;
                self.close_now(i);
            }
            Some("save") => {
                self.closing = None;
                self.save(i, true);
            }
            Some(_) => {
                self.closing = None;
                self.focus_editor = true;
            }
            None => {}
        }
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let title = self.docs[self.tab].title();
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    /// In screenshot mode: waits for the text, saves the window to
    /// `TEXT_EDITOR_SCREENSHOT` and quits. `TEXT_EDITOR_FIND` searches first.
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
                .ok_or_else(|| "the screenshot has the wrong size".to_string())
                .and_then(|img| img.save(&*path).map_err(|e| e.to_string()));
            match saved {
                Ok(()) => eprintln!("saved {}", path.display()),
                Err(e) => eprintln!("text-editor: screenshot failed: {e}"),
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else if !*requested && ctx.input(|i| i.time) > 2.0 {
            *requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        } else if ctx.input(|i| i.time) > 0.5
            && !self.find.open
            && let Ok(query) = std::env::var("TEXT_EDITOR_FIND")
        {
            self.find.query.text = query;
            self.find.open = true;
            self.find.replacing = std::env::var_os("TEXT_EDITOR_REPLACE").is_some();
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.take_done();
        if let Some(files) = self.incoming.as_ref().and_then(|r| {
            r.try_iter().reduce(|mut a, b| {
                a.extend(b);
                a
            })
        }) {
            self.open(files);
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        if !dropped.is_empty() {
            self.open(dropped);
        }
        self.keys(&ctx);
        self.watch_files();
        self.search();
        self.update_title(&ctx);

        let custom = !self.config.system_title_bar;
        let style = self.skins[self.skin].frame_style(ui.visuals());
        let radius = if custom { frame::corner_radius(&ctx, style.radius) } else { 0 };
        let header = frame::header_frame(ui.style(), &style, radius);
        egui::Panel::top("header").frame(header).show(ui, |ui| {
            let span = frame::header_span(ui);
            self.header(ui, &style, span);
        });
        let selection = view::selection(&ctx, &self.docs[self.tab]);
        egui::Panel::bottom("status")
            .frame(
                egui::Frame::side_top_panel(ui.style())
                    .inner_margin(egui::Margin::symmetric(10, 4))
                    .corner_radius(CornerRadius { sw: radius, se: radius, ..Default::default() }),
            )
            .show(ui, |ui| self.status_bar(ui, selection));
        if self.find.open || self.goto.is_some() {
            egui::Panel::top("find")
                .frame(egui::Frame::side_top_panel(ui.style()).inner_margin(egui::Margin::symmetric(12, 6)))
                .show(ui, |ui| {
                    if self.goto.is_some() {
                        self.goto_bar(ui);
                    } else {
                        self.find_bar(ui);
                    }
                });
        }
        let look = Look {
            line_numbers: self.config.line_numbers,
            wrap: self.config.wrap,
            highlight_line: self.config.highlight_line,
            font: FontId::monospace(self.config.font_size),
        };
        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(ui.style()).fill(ui.visuals().extreme_bg_color).inner_margin(0))
            .show(ui, |ui| {
                self.notice(ui);
                let focus = std::mem::take(&mut self.focus_editor) && self.closing.is_none();
                let scroll_to = self.scroll_to.take();
                let doc = &mut self.docs[self.tab];
                if doc.loading {
                    ui.centered_and_justified(|ui| ui.spinner());
                    // Focus once it's read.
                    self.focus_editor |= focus;
                    return;
                }
                let shown = view::show(ui, doc, &look, focus, scroll_to);
                if shown.changed && self.find.open {
                    self.search();
                }
            });
        self.closing_dialog(&ctx);

        self.keep_drafts(false);
        if std::mem::take(&mut self.session_changed) && self.screenshot.is_none() {
            self.session().save();
        }
        if custom {
            frame::edges(&ctx, radius, style.border);
        }
        if ctx.input(|i| i.viewport().close_requested()) {
            self.quit();
        }
        self.take_screenshot(&ctx);
    }

    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        frame::clear_color(visuals, !self.config.system_title_bar)
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        frame::release_grab(ctx, raw_input);
        patina::ime::drop_repeated_preedits(ctx, raw_input);
        patina::kinetic::glide(ctx, raw_input);
    }

    fn persist_egui_memory(&self) -> bool {
        false
    }
}

/// A small on/off button for the find bar's options.
fn toggle(ui: &mut Ui, on: &mut bool, label: &str, tip: &str) {
    if ui.add(egui::Button::selectable(*on, label).min_size(vec2(28.0, 24.0))).on_hover_text(tip).clicked() {
        *on = !*on;
    }
}

/// Selects all of a field's text, so typing replaces it.
fn select_all(ui: &Ui, field: &egui::Response, text: &str) {
    if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), field.id) {
        let all = text.chars().count();
        state.cursor.set_char_range(Some(egui::text::CCursorRange::two(
            egui::text::CCursor::new(0),
            egui::text::CCursor::new(all),
        )));
        state.store(ui.ctx(), field.id);
    }
}

/// A message for the status bar.
#[derive(Clone)]
struct Status {
    level: Level,
    text: String,
}

#[derive(Clone, Copy)]
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

fn weak(ui: &Ui, text: &str) -> RichText {
    RichText::new(text).color(ui.visuals().weak_text_color())
}

fn load_skins() -> (Vec<Skin>, Vec<String>) {
    skin::load_all(skin::BUILT_IN, &skin::app_dir(APP).into_iter().collect::<Vec<_>>())
}
