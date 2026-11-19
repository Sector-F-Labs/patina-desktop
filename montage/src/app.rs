//! The window: Patina's frame with the menus and Export in its header bar,
//! the library on the left, the preview in the middle, the timeline along
//! the bottom over the status bar, and the dialogs. It owns the project and
//! its undo history, and applies what the panels ask for.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};

use egui::{Key, KeyboardShortcut, Modifiers, RichText};
use patina::frame::{self, TitleBar};
use patina::skin::{self, Skin};

use crate::config::{APP, Config};
use crate::library;
use crate::media::{self, Reader};
use crate::preview::Preview;
use crate::project::{MediaId, Project, Sel, Track, timecode};
use crate::record::Recording;
use crate::render::{self, Export, FPS};
use crate::timeline::{self, Colours, Edit, SkinColours, Timeline};

/// The skin used until one is picked: Default, following the desktop.
const HOME_SKIN: &str = "Default";

/// How many steps Undo remembers.
const UNDO_STEPS: usize = 200;

/// The file types offered by Import.
const MEDIA_TYPES: &[&str] = &[
    "mp4", "m4v", "mov", "mkv", "webm", "avi", "mts", "m2ts", "3gp", "ogv", "mpg", "mpeg", "wmv", "mp3", "m4a", "aac",
    "wav", "flac", "ogg", "opus", "wma", "aiff",
];

fn load_skins() -> (Vec<Skin>, Vec<String>) {
    skin::load_all(skin::BUILT_IN, &skin::app_dir(APP).into_iter().collect::<Vec<_>>())
}

/// Something asked for from a menu, the header or a shortcut, done once it's drawn.
enum Do {
    New,
    Open,
    Save,
    SaveAs,
    Import,
    Export,
    Quit,
    Undo,
    Redo,
    Split,
    Delete,
    PlayPause,
    Seek(f64),
    Step(f64),
    Record,
    ZoomIn,
    ZoomOut,
    ZoomFit,
    SetSkin(String),
    SetLightDark(String),
    ReloadSkins,
    CopySkins,
    OpenSkinsFolder,
    SystemTitleBar(bool),
}

/// What to do once unsaved changes are dealt with.
#[derive(Clone)]
enum Then {
    New,
    Open,
    Quit,
}

enum Dialog {
    Unsaved(Then),
    Export { size: usize },
}

/// What a file dialog, run off the UI thread, came back with.
enum Picked {
    Import(Vec<PathBuf>),
    Open(PathBuf),
    SaveAs(PathBuf, Option<Then>),
    ExportTo(PathBuf, usize),
}

/// What a dialog's button does once the dialog is drawn.
type Later = Box<dyn FnOnce(&mut App)>;

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

pub struct App {
    config: Config,
    skins: Vec<Skin>,
    skin_errors: Vec<String>,
    forced_skin: Option<String>,
    colours: SkinColours,

    project: Project,
    path: Option<PathBuf>,
    dirty: bool,
    undo: Vec<Project>,
    redo: Vec<Project>,
    /// Counts every change, so the preview knows to look again.
    revision: u64,

    reader: Reader,
    /// Files sent to the reader and not back yet.
    reading: usize,
    thumbs: HashMap<MediaId, egui::TextureHandle>,
    peaks: HashMap<MediaId, Vec<f32>>,
    media_errors: HashMap<MediaId, String>,
    /// Why ffmpeg can't be used, if it can't.
    no_ffmpeg: Option<String>,

    preview: Preview,
    playhead: f64,
    /// The playhead and revision of the last still asked for.
    still_for: Option<(f64, u64)>,
    selected: Option<Sel>,
    timeline: Timeline,

    recording: Option<Recording>,
    /// A recording being finished, and where it goes in the movie.
    finishing: Option<(Receiver<Option<PathBuf>>, f64)>,
    /// Recordings to put on the voiceover track once they've been read.
    place_voice: Vec<(MediaId, f64)>,
    export: Option<Export>,

    dialog: Option<Dialog>,
    picking: Option<Receiver<Picked>>,
    status: Option<Status>,
    /// Set once unsaved changes have been dealt with, so the window may close.
    may_close: bool,
    title: String,
    /// Screenshot mode: where to save, and whether it has been asked for.
    screenshot: Option<(PathBuf, bool)>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, open: Option<PathBuf>) -> Self {
        patina::appearance::watch(&cc.egui_ctx);
        patina::emoji::install(patina_emoji_font::FONT);
        let config = Config::load();
        let (skins, skin_errors) = load_skins();
        let mut app = App {
            forced_skin: std::env::var("MONTAGE_SKIN").ok(),
            colours: SkinColours::default(),
            skins,
            skin_errors,
            project: Project::default(),
            path: None,
            dirty: false,
            undo: Vec::new(),
            redo: Vec::new(),
            revision: 0,
            reader: Reader::start(&cc.egui_ctx),
            reading: 0,
            thumbs: HashMap::new(),
            peaks: HashMap::new(),
            media_errors: HashMap::new(),
            no_ffmpeg: None,
            preview: Preview::new(&cc.egui_ctx),
            playhead: 0.0,
            still_for: None,
            selected: None,
            timeline: Timeline::default(),
            recording: None,
            finishing: None,
            place_voice: Vec::new(),
            export: None,
            dialog: None,
            picking: None,
            status: None,
            may_close: false,
            title: String::new(),
            screenshot: std::env::var_os("MONTAGE_SCREENSHOT").map(|p| (PathBuf::from(p), false)),
            config,
        };
        app.apply_skin(&cc.egui_ctx);
        // What the command line names, or else the project open last time.
        if let Some(path) = open.or_else(|| app.config.project.clone().filter(|p| p.exists())) {
            app.open_path(path);
        }
        app
    }

    fn skin(&self) -> &Skin {
        let find = |name: &str| self.skins.iter().find(|s| s.name == name);
        let chosen = self.forced_skin.as_deref().or(self.config.skin.as_deref()).unwrap_or(HOME_SKIN);
        find(chosen).or_else(|| find(HOME_SKIN)).unwrap_or(&self.skins[0])
    }

    fn apply_skin(&mut self, ctx: &egui::Context) {
        let skin = self.skin().clone();
        skin.apply(ctx);
        if skin.follows_desktop() {
            ctx.set_theme(self.config.theme());
        }
        self.colours = match skin.app_section::<SkinColours>(APP) {
            Ok((colours, warnings)) => {
                self.skin_errors.extend(warnings.into_iter().map(|w| format!("{}: {w}", skin.name)));
                colours
            }
            Err(e) => {
                self.skin_errors.push(format!("{}: {e}", skin.name));
                SkinColours::default()
            }
        };
    }

    fn timeline_colours(&self, v: &egui::Visuals) -> Colours {
        Colours::new(&self.colours, v)
    }

    // The project.

    /// Keeps the project as it is now for Undo, before a change.
    fn snapshot(&mut self) {
        self.undo.push(self.project.clone());
        if self.undo.len() > UNDO_STEPS {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    fn changed(&mut self) {
        self.revision += 1;
        self.dirty = true;
        if self.preview.is_playing() && self.recording.is_none() {
            self.preview.stop();
        }
        self.playhead = self.playhead.min(self.project.extent());
        if let Some(sel) = self.selected
            && self.project.clip(sel).is_none()
        {
            self.selected = None;
        }
    }

    fn undo(&mut self) {
        if let Some(before) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.project, before));
            self.selected = None;
            self.changed();
        }
    }

    fn redo(&mut self) {
        if let Some(after) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.project, after));
            self.selected = None;
            self.changed();
        }
    }

    /// Starts reading every file in the library again, as after opening a project.
    fn read_all(&mut self) {
        self.thumbs.clear();
        self.peaks.clear();
        self.media_errors.clear();
        for m in &self.project.media {
            self.reader.read(m.id, m.path.clone());
            self.reading += 1;
        }
    }

    fn import(&mut self, paths: Vec<PathBuf>) {
        let mut added = Vec::new();
        let before = self.project.clone();
        for path in paths.into_iter().filter(|p| p.is_file()) {
            let (id, new) = self.project.add_media(path.clone());
            if new {
                added.push((id, path));
            }
        }
        if added.is_empty() {
            return;
        }
        self.undo.push(before);
        self.redo.clear();
        let n = added.len();
        for (id, path) in added {
            self.reader.read(id, path);
            self.reading += 1;
        }
        self.changed();
        self.status = Some(Status::info(format!("Imported {n} file{}", if n == 1 { "" } else { "s" })));
    }

    fn open_path(&mut self, path: PathBuf) {
        match Project::load(&path) {
            Ok(project) => {
                self.preview.stop();
                self.project = project;
                self.undo.clear();
                self.redo.clear();
                self.selected = None;
                self.playhead = 0.0;
                self.dirty = false;
                self.revision += 1;
                self.read_all();
                let missing = self.project.media.iter().filter(|m| !m.path.exists()).count();
                self.status = Some(if missing > 0 {
                    Status::error(format!(
                        "{missing} file{} in this project can't be found",
                        if missing == 1 { "" } else { "s" }
                    ))
                } else {
                    Status::info(format!("Opened {}", path.display()))
                });
                self.config.project = Some(path.clone());
                self.config.save();
                self.path = Some(path);
                self.timeline.fit(self.project.extent());
            }
            Err(e) => self.status = Some(Status::error(format!("Couldn't open {}: {e}", path.display()))),
        }
    }

    fn new_project(&mut self) {
        self.preview.stop();
        self.project = Project::default();
        self.path = None;
        self.undo.clear();
        self.redo.clear();
        self.selected = None;
        self.playhead = 0.0;
        self.dirty = false;
        self.revision += 1;
        self.read_all();
        self.config.project = None;
        self.config.save();
    }

    /// Saves to `path`; then does `then`, if it's given.
    fn save_to(&mut self, path: PathBuf, then: Option<Then>, ctx: &egui::Context) {
        let path = if path.extension().is_none() { path.with_extension("montage") } else { path };
        match self.project.save(&path) {
            Ok(()) => {
                self.dirty = false;
                self.status = Some(Status::info(format!("Saved {}", path.display())));
                self.config.project = Some(path.clone());
                self.config.save();
                self.path = Some(path);
                if let Some(then) = then {
                    self.then(then, ctx);
                }
            }
            Err(e) => self.status = Some(Status::error(format!("Couldn't save: {e}"))),
        }
    }

    fn save(&mut self, then: Option<Then>, ctx: &egui::Context) {
        match self.path.clone() {
            Some(path) => self.save_to(path, then, ctx),
            None => self.save_as(then),
        }
    }

    fn save_as(&mut self, then: Option<Then>) {
        let name = self.name();
        let dir = self.path.as_ref().and_then(|p| p.parent().map(Path::to_path_buf)).or_else(dirs::video_dir);
        self.pick(move || {
            let mut d = rfd::FileDialog::new().set_title("Save Project").add_filter("Montage project", &["montage"]);
            d = d.set_file_name(format!("{name}.montage"));
            if let Some(dir) = dir {
                d = d.set_directory(dir);
            }
            d.save_file().map(|p| Picked::SaveAs(p, then))
        });
    }

    /// Does `then` now if there are no unsaved changes, or asks first.
    fn after_saving(&mut self, then: Then, ctx: &egui::Context) {
        if self.dirty {
            self.dialog = Some(Dialog::Unsaved(then));
        } else {
            self.then(then, ctx);
        }
    }

    fn then(&mut self, then: Then, ctx: &egui::Context) {
        match then {
            Then::New => self.new_project(),
            Then::Open => self.pick(|| {
                rfd::FileDialog::new()
                    .set_title("Open Project")
                    .add_filter("Montage project", &["montage"])
                    .pick_file()
                    .map(Picked::Open)
            }),
            Then::Quit => {
                self.may_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    /// Runs a file dialog off the UI thread.
    fn pick(&mut self, dialog: impl FnOnce() -> Option<Picked> + Send + 'static) {
        if self.picking.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            if let Some(picked) = dialog() {
                let _ = tx.send(picked);
            }
        });
        self.picking = Some(rx);
    }

    fn name(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".into())
    }

    // Playing and recording.

    fn play_pause(&mut self, ctx: &egui::Context) {
        if self.recording.is_some() {
            self.stop_recording(ctx);
        } else if self.preview.is_playing() {
            self.preview.stop();
        } else {
            // From the start again once it has played to the end.
            if self.playhead >= self.project.duration() - 0.05 {
                self.playhead = 0.0;
            }
            if let Err(e) = self.preview.play(&self.project, self.playhead, false, ctx) {
                self.status = Some(Status::error(e));
            }
        }
    }

    fn record(&mut self, ctx: &egui::Context) {
        if self.recording.is_some() {
            self.stop_recording(ctx);
            return;
        }
        if self.finishing.is_some() {
            return;
        }
        match Recording::start(self.playhead) {
            Ok(recording) => {
                self.recording = Some(recording);
                // The movie plays without its sound, so the microphone hears only the voice.
                if !self.project.video.is_empty() && self.playhead < self.project.duration() {
                    let _ = self.preview.play(&self.project, self.playhead, true, ctx);
                }
                self.status = Some(Status::info("Recording a voiceover: press Space or Stop to finish"));
            }
            Err(e) => self.status = Some(Status::error(e)),
        }
    }

    fn stop_recording(&mut self, ctx: &egui::Context) {
        if let Some(recording) = self.recording.take() {
            self.preview.stop();
            let at = recording.at;
            self.finishing = Some((recording.stop(ctx), at));
        }
    }

    // Applying what was asked for.

    fn act(&mut self, ctx: &egui::Context, action: Do) {
        match action {
            Do::New => self.after_saving(Then::New, ctx),
            Do::Open => self.after_saving(Then::Open, ctx),
            Do::Save => self.save(None, ctx),
            Do::SaveAs => self.save_as(None),
            Do::Import => self.pick(|| {
                rfd::FileDialog::new()
                    .set_title("Import Media")
                    .add_filter("Video and sound", MEDIA_TYPES)
                    .add_filter("All files", &["*"])
                    .pick_files()
                    .map(Picked::Import)
            }),
            Do::Export => {
                if self.project.video.is_empty() {
                    self.status = Some(Status::error("Put some clips on the video track to make a movie"));
                } else if self.export.is_none() {
                    let size = render::SIZES.iter().position(|(n, _)| *n == self.config.export_size).unwrap_or(2);
                    self.dialog = Some(Dialog::Export { size });
                }
            }
            Do::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Do::Undo => self.undo(),
            Do::Redo => self.redo(),
            Do::Split => {
                let sel = self.selected.or_else(|| {
                    self.project.video_at(self.playhead).map(|(index, _)| Sel { track: Track::Video, index })
                });
                if let Some(sel) = sel {
                    self.apply(ctx, Edit::Split(sel));
                }
            }
            Do::Delete => {
                if let Some(sel) = self.selected {
                    self.apply(ctx, Edit::Remove(sel));
                }
            }
            Do::PlayPause => self.play_pause(ctx),
            Do::Seek(t) => {
                self.preview.stop();
                self.playhead = t.clamp(0.0, self.project.extent());
            }
            Do::Step(by) => {
                self.preview.stop();
                // To the frame before or after.
                let frame = (self.playhead * FPS as f64).round() + by * FPS as f64;
                self.playhead = (frame / FPS as f64).clamp(0.0, self.project.extent());
            }
            Do::Record => self.record(ctx),
            Do::ZoomIn => self.timeline.zoom_by(1.5),
            Do::ZoomOut => self.timeline.zoom_by(1.0 / 1.5),
            Do::ZoomFit => self.timeline.fit(self.project.extent()),
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

    /// Applies what the timeline asked for.
    fn apply(&mut self, ctx: &egui::Context, edit: Edit) {
        match edit {
            Edit::Seek(t) => self.act(ctx, Do::Seek(t)),
            Edit::Select(sel) => self.selected = sel,
            Edit::Snapshot => self.snapshot(),
            Edit::Changed => self.changed(),
            Edit::MoveVideo { from, to } => {
                self.snapshot();
                let index = self.project.move_video(from, to);
                self.selected = Some(Sel { track: Track::Video, index });
                self.changed();
            }
            Edit::Drop { track, media, at } => {
                let before = self.project.clone();
                match self.project.drop_media(track, media, at) {
                    Ok(sel) => {
                        self.undo.push(before);
                        self.redo.clear();
                        self.selected = Some(sel);
                        self.changed();
                    }
                    Err(e) => self.status = Some(Status::error(e)),
                }
            }
            Edit::Split(sel) => {
                let before = self.project.clone();
                let split = match sel.track {
                    Track::Video => match self.project.video_at(self.playhead) {
                        Some((i, _)) if i == sel.index => self.project.split_video(self.playhead),
                        _ => None,
                    },
                    _ => self.project.split_placed(sel.track, sel.index, self.playhead),
                };
                match split {
                    Some(index) => {
                        self.undo.push(before);
                        self.redo.clear();
                        self.selected = Some(Sel { index, ..sel });
                        self.changed();
                    }
                    None => self.status = Some(Status::error("Put the playhead inside the clip to split it there")),
                }
            }
            Edit::Remove(sel) => {
                self.snapshot();
                self.project.remove(sel);
                self.selected = None;
                self.changed();
            }
            Edit::Volume(sel, volume) => {
                self.snapshot();
                if let Some(clip) = self.project.clip_mut(sel) {
                    clip.volume = volume;
                }
                self.changed();
            }
        }
    }

    fn apply_library(&mut self, ctx: &egui::Context, action: library::Action) {
        match action {
            library::Action::Import => self.act(ctx, Do::Import),
            library::Action::Add(media, track) => {
                let at = match track {
                    Track::Video => self.project.duration(),
                    _ => self.playhead,
                };
                self.apply(ctx, Edit::Drop { track, media, at });
            }
            library::Action::Remove(media) => {
                self.snapshot();
                self.project.remove_media(media);
                self.selected = None;
                self.changed();
            }
            library::Action::Reveal(path) => {
                if let Some(dir) = path.parent() {
                    let _ = patina::open::path(dir);
                }
            }
        }
    }

    // What arrives from the background.

    fn poll(&mut self, ctx: &egui::Context) {
        for update in self.reader.poll() {
            match update {
                media::Update::Probed { id, info, thumb } => {
                    self.reading = self.reading.saturating_sub(1);
                    match info {
                        Ok(info) => {
                            if let Some(m) = self.project.media_mut(id) {
                                m.info = Some(info);
                            }
                            self.media_errors.remove(&id);
                        }
                        Err(e) => {
                            self.media_errors.insert(id, e);
                        }
                    }
                    if let Some(image) = thumb {
                        let texture = ctx.load_texture(format!("thumb {id}"), image, egui::TextureOptions::LINEAR);
                        self.thumbs.insert(id, texture);
                    }
                    if let Some(k) = self.place_voice.iter().position(|(m, _)| *m == id) {
                        let (media, at) = self.place_voice.remove(k);
                        self.apply(ctx, Edit::Drop { track: Track::Voice, media, at });
                    }
                    // A new length can move what's under the playhead.
                    self.revision += 1;
                }
                media::Update::Peaks { id, peaks } => {
                    self.peaks.insert(id, peaks);
                }
                media::Update::Missing(e) => self.no_ffmpeg = Some(e),
            }
        }
        if let Some(picked) = self.picking.as_ref().and_then(|rx| match rx.try_recv() {
            Ok(p) => Some(Some(p)),
            Err(mpsc::TryRecvError::Disconnected) => Some(None),
            Err(mpsc::TryRecvError::Empty) => None,
        }) {
            self.picking = None;
            match picked {
                Some(Picked::Import(paths)) => self.import(paths),
                Some(Picked::Open(path)) => self.open_path(path),
                Some(Picked::SaveAs(path, then)) => self.save_to(path, then, ctx),
                Some(Picked::ExportTo(path, size)) => self.start_export(path, size, ctx),
                None => {}
            }
        }
        if let Some(Ok(path)) = self.finishing.as_ref().map(|(rx, _)| rx.try_recv()) {
            let at = self.finishing.take().map_or(0.0, |(_, at)| at);
            match path {
                Some(path) => {
                    let (id, _) = self.project.add_media(path.clone());
                    self.reader.read(id, path);
                    self.reading += 1;
                    self.place_voice.push((id, at));
                    self.status = Some(Status::info("Voiceover recorded"));
                }
                None => self.status = Some(Status::error("Nothing was recorded: is a microphone connected?")),
            }
        }
        if let Some(recording) = &mut self.recording
            && !recording.running()
        {
            // ffmpeg gave up: no microphone, most likely.
            self.stop_recording(ctx);
        }
        if let Some(result) = self.export.as_ref().and_then(Export::poll) {
            let path = self.export.take().map(|e| e.path.clone()).unwrap_or_default();
            self.status = Some(match result {
                Ok(()) => Status::info(format!("Exported {}", path.display())),
                Err(e) => Status::error(format!("Export failed: {e}")),
            });
        }
        if let Some(t) = self.preview.advance(ctx) {
            self.playhead = t;
        } else if self.recording.is_none() && !self.preview.is_playing() {
            // Stopped: show the frame under the playhead, once for each place and change.
            let key = (self.playhead, self.revision);
            if self.still_for != Some(key) {
                self.still_for = Some(key);
                self.preview.still(&self.project, self.playhead);
            }
        }
        if self.recording.is_some() && !self.preview.is_playing() {
            // Past the movie's end (or with no movie), the playhead still moves with the voice.
            if let Some(r) = &self.recording {
                self.playhead = r.at + r.started.elapsed().as_secs_f64();
            }
            ctx.request_repaint();
        }
        self.preview.poll(ctx);
    }

    fn start_export(&mut self, path: PathBuf, size: usize, ctx: &egui::Context) {
        let path = if path.extension().is_none() { path.with_extension("mp4") } else { path };
        let (name, dims) = render::SIZES[size.min(render::SIZES.len() - 1)];
        self.config.export_size = name.into();
        self.config.save();
        match Export::start(&self.project, dims, path, ctx) {
            Ok(export) => self.export = Some(export),
            Err(e) => self.status = Some(Status::error(e)),
        }
    }

    // Drawing.

    fn menus(&self, ui: &mut egui::Ui, actions: &mut Vec<Do>) {
        fn item(ui: &mut egui::Ui, actions: &mut Vec<Do>, enabled: bool, label: &str, keys: &str, action: Do) {
            if ui.add_enabled(enabled, egui::Button::new(label).shortcut_text(keys)).clicked() {
                actions.push(action);
                ui.close();
            }
        }
        let has_video = !self.project.video.is_empty();
        ui.menu_button("File", |ui| {
            item(ui, actions, true, "New Project", "Ctrl+N", Do::New);
            item(ui, actions, true, "Open Project…", "Ctrl+O", Do::Open);
            item(ui, actions, true, "Save", "Ctrl+S", Do::Save);
            item(ui, actions, true, "Save As…", "Ctrl+Shift+S", Do::SaveAs);
            ui.separator();
            item(ui, actions, true, "Import Media…", "Ctrl+I", Do::Import);
            item(ui, actions, has_video && self.export.is_none(), "Export Movie…", "Ctrl+E", Do::Export);
            ui.separator();
            item(ui, actions, true, "Quit", "Ctrl+Q", Do::Quit);
        });
        ui.menu_button("Edit", |ui| {
            item(ui, actions, !self.undo.is_empty(), "Undo", "Ctrl+Z", Do::Undo);
            item(ui, actions, !self.redo.is_empty(), "Redo", "Ctrl+Shift+Z", Do::Redo);
            ui.separator();
            item(ui, actions, has_video || self.selected.is_some(), "Split at Playhead", "Ctrl+B", Do::Split);
            item(ui, actions, self.selected.is_some(), "Delete Clip", "Delete", Do::Delete);
        });
        ui.menu_button("View", |ui| {
            item(ui, actions, true, "Zoom In", "Ctrl++", Do::ZoomIn);
            item(ui, actions, true, "Zoom Out", "Ctrl+-", Do::ZoomOut);
            item(ui, actions, true, "Zoom to Fit", "Ctrl+0", Do::ZoomFit);
            ui.separator();
            let mut system = self.config.system_title_bar;
            if ui.checkbox(&mut system, "System Title Bar").changed() {
                actions.push(Do::SystemTitleBar(system));
            }
        });
        ui.menu_button("Playback", |ui| {
            let playing = self.preview.is_playing() && self.recording.is_none();
            item(ui, actions, has_video, if playing { "Pause" } else { "Play" }, "Space", Do::PlayPause);
            item(ui, actions, true, "Go to Start", "Home", Do::Seek(0.0));
            item(ui, actions, true, "Go to End", "End", Do::Seek(self.project.duration()));
            item(ui, actions, true, "Previous Frame", "←", Do::Step(-1.0 / FPS as f64));
            item(ui, actions, true, "Next Frame", "→", Do::Step(1.0 / FPS as f64));
            ui.separator();
            let label = if self.recording.is_some() { "Stop Recording" } else { "Record Voiceover" };
            item(ui, actions, self.finishing.is_none(), label, "Ctrl+R", Do::Record);
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

    fn shortcuts(&self, ctx: &egui::Context) -> Vec<Do> {
        let mut actions = Vec::new();
        // Keys typed into a text field are the field's.
        let typing = ctx.memory(|m| m.focused().is_some()) && ctx.egui_wants_keyboard_input();
        ctx.input_mut(|i| {
            let cmd = |k| KeyboardShortcut::new(Modifiers::COMMAND, k);
            let cmd_shift = |k| KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, k);
            for (shortcut, action) in [
                (cmd_shift(Key::S), Do::SaveAs),
                (cmd_shift(Key::Z), Do::Redo),
                (cmd(Key::N), Do::New),
                (cmd(Key::O), Do::Open),
                (cmd(Key::S), Do::Save),
                (cmd(Key::I), Do::Import),
                (cmd(Key::E), Do::Export),
                (cmd(Key::Q), Do::Quit),
                (cmd(Key::Z), Do::Undo),
                (cmd(Key::Y), Do::Redo),
                (cmd(Key::B), Do::Split),
                (cmd(Key::R), Do::Record),
                (cmd(Key::Plus), Do::ZoomIn),
                (cmd(Key::Equals), Do::ZoomIn),
                (cmd(Key::Minus), Do::ZoomOut),
                (cmd(Key::Num0), Do::ZoomFit),
            ] {
                if i.consume_shortcut(&shortcut) {
                    actions.push(action);
                }
            }
            if typing {
                return;
            }
            let none = Modifiers::NONE;
            if i.consume_key(none, Key::Space) {
                actions.push(Do::PlayPause);
            }
            if i.consume_key(none, Key::Delete) || i.consume_key(none, Key::Backspace) {
                actions.push(Do::Delete);
            }
            if i.consume_key(none, Key::Home) {
                actions.push(Do::Seek(0.0));
            }
            if i.consume_key(none, Key::End) {
                actions.push(Do::Seek(self.project.duration()));
            }
            let frame = 1.0 / FPS as f64;
            for (mods, key, by) in [
                (none, Key::ArrowLeft, -frame),
                (none, Key::ArrowRight, frame),
                (Modifiers::SHIFT, Key::ArrowLeft, -1.0),
                (Modifiers::SHIFT, Key::ArrowRight, 1.0),
            ] {
                if i.consume_key(mods, key) {
                    actions.push(Do::Step(by));
                }
            }
        });
        actions
    }

    /// The preview, letterboxed into the panel, with the transport under it.
    fn viewer(&mut self, ui: &mut egui::Ui) -> Vec<Do> {
        let mut actions = Vec::new();
        let v = ui.visuals().clone();
        let transport = 44.0;
        let space = ui.available_rect_before_wrap();
        let room = egui::vec2(space.width() - 24.0, space.height() - transport - 16.0).max(egui::vec2(16.0, 9.0));
        let size = if room.x / room.y > 16.0 / 9.0 {
            egui::vec2(room.y * 16.0 / 9.0, room.y)
        } else {
            egui::vec2(room.x, room.x * 9.0 / 16.0)
        };
        let screen = egui::Rect::from_center_size(egui::pos2(space.center().x, space.top() + 8.0 + room.y / 2.0), size);
        ui.painter().rect_filled(screen, 4.0, v.extreme_bg_color);
        match self.preview.texture() {
            Some(texture) if !self.project.video.is_empty() => {
                ui.painter().image(
                    texture.id(),
                    screen,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
            _ => {
                let hint = if self.project.media.is_empty() {
                    "Import clips with File → Import Media, or drop them on the window"
                } else if self.project.video.is_empty() {
                    "Drag clips from the library onto the video track"
                } else {
                    ""
                };
                ui.painter().text(
                    screen.center(),
                    egui::Align2::CENTER_CENTER,
                    hint,
                    egui::FontId::proportional(14.0),
                    v.weak_text_color(),
                );
            }
        }
        if self.recording.is_some() {
            let dot = egui::pos2(screen.left() + 18.0, screen.top() + 18.0);
            ui.painter().circle_filled(dot, 6.0, v.error_fg_color);
            ui.painter().text(
                dot + egui::vec2(12.0, 0.0),
                egui::Align2::LEFT_CENTER,
                "REC",
                egui::FontId::proportional(13.0),
                v.error_fg_color,
            );
        }

        let bar = egui::Rect::from_min_size(
            egui::pos2(space.left(), screen.bottom() + 8.0),
            egui::vec2(space.width(), transport),
        );
        let mut ui =
            ui.new_child(egui::UiBuilder::new().max_rect(bar).layout(egui::Layout::left_to_right(egui::Align::Center)));
        let has_video = !self.project.video.is_empty();
        let playing = self.preview.is_playing() && self.recording.is_none();
        let duration = self.project.duration();
        let time = format!("{} / {}", timecode(self.playhead), timecode(duration));
        // Centred: the buttons and the time, measured first.
        let widths = 3.0 * 36.0 + 150.0;
        ui.add_space(((bar.width() - widths) / 2.0).max(8.0));
        let big = |label: &str| egui::Button::new(RichText::new(label).size(16.0)).min_size(egui::vec2(32.0, 28.0));
        if ui.add(big("⏮")).on_hover_text("Go to start (Home)").clicked() {
            actions.push(Do::Seek(0.0));
        }
        if ui
            .add_enabled(
                has_video || self.recording.is_some(),
                big(if playing || self.recording.is_some() { "⏸" } else { "▶" }),
            )
            .on_hover_text("Play or pause (Space)")
            .clicked()
        {
            actions.push(Do::PlayPause);
        }
        if ui.add(big("⏭")).on_hover_text("Go to end (End)").clicked() {
            actions.push(Do::Seek(duration));
        }
        ui.add_space(8.0);
        ui.label(RichText::new(time).monospace());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(12.0);
            match &self.recording {
                Some(r) => {
                    let label = format!("⏹ Stop Recording  {}", timecode(r.started.elapsed().as_secs_f64()));
                    if ui.add(egui::Button::new(RichText::new(label).color(v.error_fg_color))).clicked() {
                        actions.push(Do::Record);
                    }
                }
                None => {
                    // The dot in the error colour, as a recording light.
                    let mut label = egui::text::LayoutJob::default();
                    let font = egui::TextStyle::Button.resolve(ui.style());
                    let text = |color| egui::TextFormat { font_id: font.clone(), color, ..Default::default() };
                    label.append("⏺ ", 0.0, text(v.error_fg_color));
                    label.append("Record Voiceover", 0.0, text(v.text_color()));
                    let button = egui::Button::new(label);
                    if ui
                        .add_enabled(self.finishing.is_none(), button)
                        .on_hover_text(
                            "Records from the microphone at the playhead while the movie plays silently (Ctrl+R)",
                        )
                        .clicked()
                    {
                        actions.push(Do::Record);
                    }
                }
            }
        });
        actions
    }

    /// The bar above the timeline: editing buttons on the left, zoom on the right.
    fn timeline_bar(&self, ui: &mut egui::Ui) -> Vec<Do> {
        let mut actions = Vec::new();
        ui.horizontal(|ui| {
            let has = self.selected.is_some();
            if ui
                .add_enabled(has || !self.project.video.is_empty(), egui::Button::new("Split"))
                .on_hover_text("Split the clip at the playhead (Ctrl+B)")
                .clicked()
            {
                actions.push(Do::Split);
            }
            if ui
                .add_enabled(has, egui::Button::new("Delete"))
                .on_hover_text("Delete the selected clip (Delete)")
                .clicked()
            {
                actions.push(Do::Delete);
            }
            if let Some(clip) = self.selected.and_then(|s| self.project.clip(s)) {
                let name = self.project.media(clip.media).map(|m| m.name()).unwrap_or_default();
                ui.separator();
                ui.label(RichText::new(format!("{name} — {}", timecode(clip.length))).weak());
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Fit").on_hover_text("Zoom to fit (Ctrl+0)").clicked() {
                    actions.push(Do::ZoomFit);
                }
                if ui.button("+").on_hover_text("Zoom in (Ctrl++, or Ctrl and the wheel)").clicked() {
                    actions.push(Do::ZoomIn);
                }
                if ui.button("−").on_hover_text("Zoom out (Ctrl+-)").clicked() {
                    actions.push(Do::ZoomOut);
                }
            });
        });
        actions
    }

    /// Patina's status bar (its ADR-0016): the version, then the latest message; on the right,
    /// an export under way.
    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("v{}", env!("CARGO_PKG_VERSION"))).weak().small());
            let skin_errors = (!self.skin_errors.is_empty()).then(|| {
                let n = self.skin_errors.len();
                Status::error(format!("Couldn't load {n} skin{}", if n == 1 { "" } else { "s" }))
            });
            let missing = self.no_ffmpeg.as_ref().map(Status::error);
            let sound = self.preview.no_sound.as_ref().filter(|_| self.status.is_none()).map(Status::error);
            if let Some(status) = missing.as_ref().or(skin_errors.as_ref()).or(self.status.as_ref()).or(sound.as_ref())
            {
                ui.separator();
                let color = if status.error { ui.visuals().error_fg_color } else { ui.visuals().weak_text_color() };
                let resp = ui.label(RichText::new(&status.text).color(color).small());
                if skin_errors.is_some() && missing.is_none() {
                    resp.on_hover_text(self.skin_errors.join("\n"));
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(export) = &mut self.export {
                    if ui.small_button("Cancel").clicked() {
                        export.cancel();
                    }
                    ui.add(egui::ProgressBar::new(export.progress()).desired_width(140.0).show_percentage());
                    ui.label(RichText::new("Exporting").weak().small())
                        .on_hover_text(export.path.display().to_string());
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
                } else if self.reading > 0 {
                    ui.spinner();
                    ui.label(RichText::new("Reading files…").weak().small());
                }
            });
        });
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.dialog.take() else { return };
        let mut keep = true;
        let mut after: Option<Later> = None;
        match dialog {
            Dialog::Unsaved(then) => {
                let name = self.name();
                egui::Modal::new(egui::Id::new("unsaved")).show(ctx, |ui| {
                    ui.set_width(340.0);
                    ui.heading(format!("Save changes to {name}?"));
                    ui.add_space(4.0);
                    ui.label("Your changes will be lost if you don't save them.");
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
                            keep = false;
                            let then = then.clone();
                            let ctx = ctx.clone();
                            after = Some(Box::new(move |app| app.save(Some(then), &ctx)));
                        }
                        if ui.button("Don't Save").clicked() {
                            keep = false;
                            let then = then.clone();
                            let ctx = ctx.clone();
                            after = Some(Box::new(move |app| {
                                app.dirty = false;
                                app.then(then, &ctx)
                            }));
                        }
                        if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
                            keep = false;
                        }
                    });
                });
                if keep {
                    self.dialog = Some(Dialog::Unsaved(then));
                }
            }
            Dialog::Export { mut size } => {
                let name = self.name();
                let duration = self.project.duration();
                egui::Modal::new(egui::Id::new("export")).show(ctx, |ui| {
                    ui.set_width(320.0);
                    ui.heading("Export Movie");
                    ui.add_space(4.0);
                    ui.label(format!("{} long, as an MP4 file (H.264 and AAC).", timecode(duration)));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.label("Size:");
                        for (i, (label, _)) in render::SIZES.iter().enumerate() {
                            ui.radio_value(&mut size, i, *label);
                        }
                    });
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if ui.button("Export…").clicked() {
                            keep = false;
                            let dir = dirs::video_dir();
                            after = Some(Box::new(move |app| {
                                app.pick(move || {
                                    let mut d = rfd::FileDialog::new()
                                        .set_title("Export Movie")
                                        .add_filter("MP4 video", &["mp4"])
                                        .set_file_name(format!("{name}.mp4"));
                                    if let Some(dir) = dir {
                                        d = d.set_directory(dir);
                                    }
                                    d.save_file().map(|p| Picked::ExportTo(p, size))
                                })
                            }));
                        }
                        if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
                            keep = false;
                        }
                    });
                });
                if keep {
                    self.dialog = Some(Dialog::Export { size });
                }
            }
        }
        if let Some(after) = after {
            after(self);
        }
    }

    fn take_screenshot(&mut self, ctx: &egui::Context) {
        let Some((path, asked)) = &mut self.screenshot else { return };
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
        } else if !*asked && self.reading == 0 && ctx.input(|i| i.time) > 2.5 {
            *asked = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll(ctx);
        if self.dialog.is_none() {
            for action in self.shortcuts(ctx) {
                self.act(ctx, action);
            }
        }
        // Files dropped on the window go into the library.
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        if !dropped.is_empty() {
            self.import(dropped);
        }
        if ctx.input(|i| i.viewport().close_requested()) && self.dirty && !self.may_close && self.screenshot.is_none() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.dialog = Some(Dialog::Unsaved(Then::Quit));
        }
        let title = format!("{}{} — Montage", self.name(), if self.dirty { " •" } else { "" });
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        if !self.config.system_title_bar {
            patina::frame::shadow::request(frame);
        }
        let ctx = ui.ctx().clone();
        let custom = !self.config.system_title_bar;
        let style = self.skin().frame_style(ui.visuals());
        let radius = if custom { frame::corner_radius(&ctx, style.radius) } else { 0 };
        let colours = self.timeline_colours(ui.visuals());

        let mut actions = Vec::new();
        let mut export = false;
        let title = format!("{}{}", self.name(), if self.dirty { " •" } else { "" });
        egui::Panel::top("header").frame(frame::header_frame(ui.style(), &style, radius)).show(ui, |ui| {
            TitleBar::new(&title, &style).custom(custom).menu_bar(true).span(frame::header_span(ui)).show(
                ui,
                |ui| self.menus(ui, &mut actions),
                |ui| {
                    // The app's output, next to the window buttons (Patina's ADR-0006).
                    let enabled = !self.project.video.is_empty() && self.export.is_none();
                    export = ui
                        .add_enabled(enabled, egui::Button::new("Export"))
                        .on_hover_text("Export the movie (Ctrl+E)")
                        .clicked();
                },
            );
        });
        if export {
            actions.push(Do::Export);
        }

        // It spans the window, so it comes first, and holds both bottom corners.
        egui::Panel::bottom("status")
            .frame(
                egui::Frame::side_top_panel(ui.style())
                    .inner_margin(egui::Margin::symmetric(10, 4))
                    .corner_radius(egui::CornerRadius { sw: radius, se: radius, ..Default::default() }),
            )
            .show(ui, |ui| self.status_bar(ui));

        let mut edits = Vec::new();
        egui::Panel::bottom("timeline")
            .frame(egui::Frame::side_top_panel(ui.style()).inner_margin(egui::Margin::symmetric(8, 6)))
            .resizable(false)
            .show(ui, |ui| {
                actions.extend(self.timeline_bar(ui));
                ui.add_space(4.0);
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    self.timeline.headers(ui, &colours);
                    let look = timeline::Look {
                        thumbs: &self.thumbs,
                        peaks: &self.peaks,
                        colours,
                        playhead: self.playhead,
                        selected: self.selected,
                        follow: self.preview.is_playing() || self.recording.is_some(),
                    };
                    edits = self.timeline.show(ui, &mut self.project, &look);
                });
            });

        let mut library_actions = Vec::new();
        egui::Panel::left("library")
            .frame(egui::Frame::side_top_panel(ui.style()).inner_margin(egui::Margin::symmetric(10, 4)))
            .default_size(310.0)
            .size_range(200.0..=600.0)
            .resizable(true)
            .show(ui, |ui| {
                let look =
                    library::Look { thumbs: &self.thumbs, peaks: &self.peaks, errors: &self.media_errors, colours };
                library_actions = library::show(ui, &self.project, &look);
            });

        egui::CentralPanel::default().frame(egui::Frame::central_panel(ui.style())).show(ui, |ui| {
            actions.extend(self.viewer(ui));
        });

        // Files being dragged over the window from a file manager.
        if ctx.input(|i| !i.raw.hovered_files.is_empty()) {
            let screen = ctx.content_rect();
            let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("drop files")));
            let v = ui.visuals();
            painter.rect_filled(screen.shrink(8.0), 12.0, v.extreme_bg_color.gamma_multiply(0.85));
            painter.text(
                screen.center(),
                egui::Align2::CENTER_CENTER,
                "Drop to add to the library",
                egui::FontId::proportional(20.0),
                v.text_color(),
            );
        }

        for edit in edits {
            self.apply(&ctx, edit);
        }
        for action in library_actions {
            self.apply_library(&ctx, action);
        }
        for action in actions {
            self.act(&ctx, action);
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

    #[test]
    fn every_built_in_skin_reads_as_montage_colours() {
        let (skins, _) = skin::load_all(skin::BUILT_IN, &[]);
        for s in skins {
            assert!(s.app_section::<SkinColours>(APP).is_ok(), "{}", s.name);
        }
    }

    #[test]
    fn the_preview_is_16_by_9() {
        assert_eq!(crate::preview::SIZE.width * 9, crate::preview::SIZE.height * 16);
    }
}
