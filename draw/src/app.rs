//! The window: Patina's frame and header bar, the toolbox, the canvas, the
//! palette and the status bar.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};

use egui::{Color32, Key, KeyboardShortcut, Modifiers, Pos2, Rect, Sense, TextureHandle, TextureOptions, Vec2};
use patina::frame::{self, TitleBar};
use patina::skin::{self, Skin};

use crate::bitmap::{Bitmap, Fill, Rgba, WHITE};
use crate::config::Config;
use crate::document::{Button, Document, Pen, Tool};
use crate::file::{self, Done};
use crate::crop::{self, Aspect, Handle, Region};
use crate::view::{self, Gesture};
use crate::{icons, text};

const APP: &str = "draw";

/// The skin used until the user picks one: stock egui, following the desktop.
const HOME_SKIN: &str = "Default";

/// Zoom steps, as a fraction of the picture's own pixels.
const ZOOMS: [f32; 12] = [0.125, 0.25, 0.5, 1.0, 2.0, 3.0, 4.0, 6.0, 8.0, 12.0, 16.0, 32.0];

const NEW_SIZE: (usize, usize) = (800, 600);

/// The foreground a picture starts with: a red, the colour that marks something on a picture.
const MARKER_RED: Rgba = [235, 40, 40, 255];

/// Which of the pen's two colours the picker edits.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Slot {
    Foreground,
    Background,
}

/// Text being typed on the picture: where it goes, what's been typed, and whether its box
/// still has to take the keyboard.
struct Typing {
    at: (i32, i32),
    content: String,
    fresh: bool,
}

/// What the picture's preview shows of the text being typed, to redraw it when that changes.
type Preview = ((i32, i32), String, u32, Rgba);

/// What the pointer is doing to the crop selection, from where it was pressed.
#[derive(Clone, Copy)]
enum CropDrag {
    /// Drawing a new one from this corner.
    New((i32, i32)),
    /// Carrying it: where in it (from its top left) it was taken hold of.
    Move((i32, i32)),
    /// Pulling a handle, from how the selection was when it was taken.
    Resize(Handle, Region),
}

/// How near a handle the pointer has to be to take it, in screen pixels.
const HANDLE_GRAB: f32 = 10.0;

/// The handles' size on screen.
const HANDLE_SIZE: f32 = 9.0;

/// What the header and menus ask for, handled after they're drawn.
enum Action {
    New,
    Open,
    Save,
    SaveAs,
    Quit,
    Undo,
    Redo,
    Copy,
    Zoom(i32),
    ActualSize,
    Fit,
    Flip { horizontal: bool },
    Rotate,
    Invert,
    Clear,
    ResizeCanvas,
    ApplyCrop,
    SwapColours,
    SetSkin(String),
    SystemTitleBar(bool),
}

/// Something to do once unsaved changes are dealt with.
#[derive(Clone)]
enum Then {
    New(usize, usize),
    Open,
    Quit,
}

enum Dialog {
    New { width: usize, height: usize },
    Resize { width: usize, height: usize },
    Unsaved(Then),
}

pub struct App {
    config: Config,
    skins: Vec<Skin>,
    skin_errors: Vec<String>,
    forced_skin: Option<String>,
    doc: Document,
    pen: Pen,
    zoom: f32,
    /// How far the picture has scrolled, as the scroll area has it; and whether it's to be put
    /// there (it was changed here) rather than left to the scroll area.
    scroll: Vec2,
    scroll_set: bool,
    /// Where the canvas is in the window, as of the last frame.
    window: Rect,
    /// The picture is being dragged about: with the middle button, or Space and the left one.
    panning: bool,
    /// Whether the picture's texture is smoothed (zoomed out) or each pixel sharp.
    smooth: bool,
    /// Fit the picture to the canvas on the next frame.
    fit: bool,
    texture: Option<TextureHandle>,
    texture_stale: bool,
    dialog: Option<Dialog>,
    status: Option<String>,
    io_tx: Sender<Done>,
    io_rx: Receiver<Done>,
    /// A save is running, so a second one waits and quitting asks first.
    saving: bool,
    /// After the save that's running, do this.
    after_save: Option<Then>,
    /// Where the pointer is over the picture, in its pixels.
    cursor: Option<(i32, i32)>,
    /// Quit once the window next asks to close: unsaved changes were dealt with.
    closing: bool,
    /// Screenshot mode: where to save it, and whether it was asked for.
    screenshot: Option<(PathBuf, bool)>,
    /// Which colour the picker edits when a tool has two ([`Slot`]): a shape's outline or its fill.
    slot: Slot,
    /// Crop: the part chosen to keep, and whether the pointer is still choosing it.
    crop: Option<Region>,
    crop_drag: Option<CropDrag>,
    /// The shape the selection is held to, and whether it's turned on its side.
    aspect: Aspect,
    portrait: bool,
    /// Text: what's being typed, and what the picture shows of it.
    typing: Option<Typing>,
    preview: Option<Preview>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, config: Config) -> Self {
        patina::appearance::watch(&cc.egui_ctx);
        let dirs: Vec<_> = skin::app_dir(APP).into_iter().collect();
        let (skins, skin_errors) = skin::load_all(skin::BUILT_IN, &dirs);
        let (io_tx, io_rx) = channel();
        let screenshot = std::env::var_os("DRAW_SCREENSHOT").map(|p| (PathBuf::from(p), false));
        let pen = Pen { tool: Tool::Rectangle, size: config.size, fill: config.fill, fg: MARKER_RED, bg: WHITE };
        let mut app = Self {
            config,
            skins,
            skin_errors,
            forced_skin: std::env::var("DRAW_SKIN").ok(),
            doc: Document::new(Bitmap::new(NEW_SIZE.0, NEW_SIZE.1, WHITE)),
            pen,
            zoom: 1.0,
            scroll: Vec2::ZERO,
            scroll_set: false,
            window: Rect::ZERO,
            panning: false,
            smooth: false,
            fit: true,
            texture: None,
            texture_stale: true,
            dialog: None,
            status: None,
            io_tx,
            io_rx,
            saving: false,
            after_save: None,
            cursor: None,
            closing: false,
            screenshot,
            slot: Slot::Foreground,
            crop: None,
            crop_drag: None,
            aspect: Aspect::Free,
            portrait: false,
            typing: None,
            preview: None,
        };
        if let Some(tool) = std::env::var("DRAW_TOOL").ok().and_then(|n| Tool::ALL.into_iter().find(|t| t.name().eq_ignore_ascii_case(&n))) {
            app.pen.tool = tool;
        }
        app.apply_skin(&cc.egui_ctx);
        if let Some(path) = std::env::args_os().nth(1) {
            file::open(&cc.egui_ctx, app.io_tx.clone(), Some(PathBuf::from(path)));
        } else if app.screenshot.is_some() {
            app.doc = Document::new(demo());
        }
        app
    }

    fn skin(&self) -> &Skin {
        let find = |name: &str| self.skins.iter().find(|s| s.name == name);
        let name = self.forced_skin.as_deref().or(self.config.skin.as_deref()).unwrap_or(HOME_SKIN);
        find(name).or_else(|| find(HOME_SKIN)).unwrap_or(&self.skins[0])
    }

    fn apply_skin(&self, ctx: &egui::Context) {
        self.skin().apply(ctx);
    }

    fn title(&self) -> String {
        let name = self.doc.path.as_deref().map(file::name).unwrap_or_else(|| "Untitled".into());
        format!("{}{name} – Draw", if self.doc.modified { "• " } else { "" })
    }

    // ----- what the user asked for -----

    fn act(&mut self, ctx: &egui::Context, action: Action) {
        match action {
            Action::New => self.ask_or(Then::New(NEW_SIZE.0, NEW_SIZE.1), ctx, true),
            Action::Open => self.ask_or(Then::Open, ctx, false),
            Action::Save => self.save(ctx, false),
            Action::SaveAs => self.save(ctx, true),
            Action::Quit => self.ask_or(Then::Quit, ctx, false),
            Action::Undo => {
                let did = self.doc.undo();
                self.changed(did);
            }
            Action::Redo => {
                let did = self.doc.redo();
                self.changed(did);
            }
            Action::Copy => {
                let b = &self.doc.bitmap;
                ctx.copy_image(egui::ColorImage::from_rgba_unmultiplied([b.width, b.height], &b.to_bytes()));
                self.status = Some("Copied the picture".into());
            }
            Action::Zoom(steps) => self.zoom_by(steps),
            Action::ActualSize => self.zoom_to(1.0),
            Action::Fit => self.fit = true,
            Action::Flip { horizontal } => {
                self.edit(|b| if horizontal { b.flip_horizontal() } else { b.flip_vertical() });
            }
            Action::Rotate => self.edit(Bitmap::rotate_clockwise),
            Action::Invert => self.edit(Bitmap::invert),
            Action::Clear => {
                let bg = self.pen.bg;
                self.edit(|b| b.clear(bg));
            }
            Action::ResizeCanvas => {
                let b = &self.doc.bitmap;
                self.dialog = Some(Dialog::Resize { width: b.width, height: b.height });
            }
            Action::ApplyCrop => {
                if let Some(r) = self.crop.take() {
                    if self.doc.crop((r.x0, r.y0), (r.x1, r.y1)) {
                        self.status = Some(format!("Cropped to {} × {}", r.width(), r.height()));
                    }
                    self.crop_drag = None;
                    self.texture_stale = true;
                    self.fit = true;
                }
            }
            Action::SwapColours => std::mem::swap(&mut self.pen.fg, &mut self.pen.bg),
            Action::SetSkin(name) => {
                self.config.skin = Some(name);
                self.config.save();
                self.apply_skin(ctx);
            }
            Action::SystemTitleBar(on) => {
                self.config.system_title_bar = on;
                self.config.save();
                ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(on));
            }
        }
    }

    fn edit(&mut self, change: impl FnOnce(&mut Bitmap)) {
        self.doc.edit(change);
        self.texture_stale = true;
    }

    fn changed(&mut self, did: bool) {
        if did {
            self.texture_stale = true;
        }
    }

    /// Zooms by `factor` about `anchor`, a point of the canvas from its top left, so what's under
    /// it stays there.
    fn zoom_at(&mut self, factor: f32, anchor: Vec2) {
        let picture = Vec2::new(self.doc.bitmap.width as f32, self.doc.bitmap.height as f32);
        let (zoom, scroll) = view::zoom_about(self.zoom, self.scroll, factor, anchor, self.window.size(), picture);
        self.zoom = zoom;
        self.scroll = scroll;
        self.scroll_set = true;
    }

    /// Zooms to `target`, about the middle of the canvas: for the keys and the menu.
    fn zoom_to(&mut self, target: f32) {
        self.zoom_at(target / self.zoom, self.window.size() / 2.0);
    }

    /// The next of the usual zoom steps up or down from the zoom now, which the wheel may have
    /// left between two.
    fn zoom_by(&mut self, steps: i32) {
        let now = self.zoom;
        let target = if steps > 0 {
            ZOOMS.iter().copied().find(|z| *z > now * 1.001).unwrap_or(view::MAX_ZOOM.min(ZOOMS[ZOOMS.len() - 1]))
        } else {
            ZOOMS.iter().copied().rev().find(|z| *z < now / 1.001).unwrap_or(ZOOMS[0])
        };
        self.zoom_to(target);
    }

    /// Does `then` now, or asks first when there are unsaved changes. A New
    /// asks for its size unless it comes from the dialog (`pick_size` false).
    fn ask_or(&mut self, then: Then, ctx: &egui::Context, pick_size: bool) {
        if self.doc.modified {
            self.dialog = Some(Dialog::Unsaved(then));
        } else {
            self.proceed(then, ctx, pick_size);
        }
    }

    fn proceed(&mut self, then: Then, ctx: &egui::Context, pick_size: bool) {
        match then {
            Then::New(width, height) if pick_size => self.dialog = Some(Dialog::New { width, height }),
            Then::New(width, height) => self.replace(Document::new(Bitmap::new(width, height, self.pen.bg))),
            Then::Open => file::open(ctx, self.io_tx.clone(), None),
            Then::Quit => {
                self.closing = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    fn replace(&mut self, doc: Document) {
        self.doc = doc;
        self.texture_stale = true;
        self.fit = true;
    }

    fn save(&mut self, ctx: &egui::Context, always_ask: bool) {
        if self.saving {
            return;
        }
        self.saving = true;
        let path = if always_ask { None } else { self.doc.path.clone() };
        // Save As starts from the picture's own folder, with its name and ` (edit)`.
        let suggest = self.doc.path.as_deref().map(file::edit_name).unwrap_or_else(|| "Untitled.png".into());
        let folder = self.doc.path.as_deref().and_then(|p| p.parent()).map(PathBuf::from);
        file::save(ctx, self.io_tx.clone(), self.doc.bitmap.clone(), path, &suggest, folder);
    }

    /// Picks up what the file thread finished.
    fn files(&mut self, ctx: &egui::Context) {
        while let Ok(done) = self.io_rx.try_recv() {
            match done {
                Done::Opened(path, bitmap) => {
                    self.status = Some(format!("Opened {}", file::name(&path)));
                    let mut doc = Document::new(bitmap);
                    doc.path = Some(path);
                    self.replace(doc);
                }
                Done::Saved(path) => {
                    self.saving = false;
                    self.status = Some(format!("Saved {}", file::name(&path)));
                    self.doc.path = Some(path);
                    self.doc.modified = false;
                    if let Some(then) = self.after_save.take() {
                        self.proceed(then, ctx, false);
                    }
                }
                Done::Failed(message) => {
                    self.saving = false;
                    self.after_save = None;
                    self.status = Some(message);
                }
                Done::Cancelled => {
                    self.saving = false;
                    self.after_save = None;
                }
            }
        }
    }

    // ----- drawing the window -----

    fn menus(&self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        ui.menu_button("File", |ui| {
            item(ui, actions, "New…", "Ctrl+N", true, Action::New);
            item(ui, actions, "Open…", "Ctrl+O", true, Action::Open);
            ui.separator();
            item(ui, actions, "Save", "Ctrl+S", true, Action::Save);
            item(ui, actions, "Save As…", "Ctrl+Shift+S", true, Action::SaveAs);
            ui.separator();
            item(ui, actions, "Quit", "Ctrl+Q", true, Action::Quit);
        });
        ui.menu_button("Edit", |ui| {
            item(ui, actions, "Undo", "Ctrl+Z", self.doc.can_undo(), Action::Undo);
            item(ui, actions, "Redo", "Ctrl+Shift+Z", self.doc.can_redo(), Action::Redo);
            ui.separator();
            item(ui, actions, "Copy Picture", "Ctrl+C", true, Action::Copy);
            ui.separator();
            item(ui, actions, "Swap Colours", "X", true, Action::SwapColours);
        });
        ui.menu_button("Image", |ui| {
            item(ui, actions, "Flip Horizontal", "", true, Action::Flip { horizontal: true });
            item(ui, actions, "Flip Vertical", "", true, Action::Flip { horizontal: false });
            item(ui, actions, "Rotate Clockwise", "Ctrl+R", true, Action::Rotate);
            item(ui, actions, "Invert Colours", "Ctrl+I", true, Action::Invert);
            ui.separator();
            item(ui, actions, "Crop to Selection", "Enter", self.crop.is_some(), Action::ApplyCrop);
            item(ui, actions, "Canvas Size…", "", true, Action::ResizeCanvas);
            item(ui, actions, "Clear", "", true, Action::Clear);
        });
        ui.menu_button("View", |ui| {
            item(ui, actions, "Zoom In", "Ctrl++", true, Action::Zoom(1));
            item(ui, actions, "Zoom Out", "Ctrl+-", true, Action::Zoom(-1));
            item(ui, actions, "Actual Size", "Ctrl+0", true, Action::ActualSize);
            item(ui, actions, "Fit to Window", "Ctrl+9", true, Action::Fit);
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

    fn shortcuts(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let ctrl = Modifiers::COMMAND;
        let shift = Modifiers::COMMAND | Modifiers::SHIFT;
        let table = [
            (KeyboardShortcut::new(ctrl, Key::N), Action::New),
            (KeyboardShortcut::new(ctrl, Key::O), Action::Open),
            (KeyboardShortcut::new(shift, Key::S), Action::SaveAs),
            (KeyboardShortcut::new(ctrl, Key::S), Action::Save),
            (KeyboardShortcut::new(ctrl, Key::Q), Action::Quit),
            (KeyboardShortcut::new(shift, Key::Z), Action::Redo),
            (KeyboardShortcut::new(ctrl, Key::Z), Action::Undo),
            (KeyboardShortcut::new(ctrl, Key::Y), Action::Redo),
            (KeyboardShortcut::new(ctrl, Key::C), Action::Copy),
            (KeyboardShortcut::new(ctrl, Key::R), Action::Rotate),
            (KeyboardShortcut::new(ctrl, Key::I), Action::Invert),
            (KeyboardShortcut::new(ctrl, Key::Plus), Action::Zoom(1)),
            (KeyboardShortcut::new(ctrl, Key::Equals), Action::Zoom(1)),
            (KeyboardShortcut::new(ctrl, Key::Minus), Action::Zoom(-1)),
            (KeyboardShortcut::new(ctrl, Key::Num0), Action::ActualSize),
            (KeyboardShortcut::new(ctrl, Key::Num9), Action::Fit),
        ];
        for (shortcut, action) in table {
            if ctx.input_mut(|i| i.consume_shortcut(&shortcut)) {
                actions.push(action);
            }
        }
        // Plain keys pick tools, unless a text field or a dialog wants them.
        if self.dialog.is_none() && !ctx.egui_wants_keyboard_input() {
            for tool in Tool::ALL {
                if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, tool.key())) {
                    self.pen.tool = tool;
                }
            }
            if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::X)) {
                actions.push(Action::SwapColours);
            }
            if self.pen.tool == Tool::Crop
                && self.crop.is_some()
                && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter))
            {
                actions.push(Action::ApplyCrop);
            }
            if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
                self.crop = None;
                self.crop_drag = None;
                self.doc.cancel();
                self.texture_stale = true;
            }
        }
    }

    /// The tools, then what the chosen one needs: its colour, its width, its shape. A tool that
    /// has no use for one doesn't show it.
    fn toolbox(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let tool = self.pen.tool;
        egui::ScrollArea::vertical().auto_shrink([true, false]).show(ui, |ui| {
            ui.add_space(4.0);
            egui::Grid::new("tools").spacing([4.0, 4.0]).show(ui, |ui| {
                for (n, t) in Tool::ALL.into_iter().enumerate() {
                    let on = tool == t;
                    let hint = format!("{} ({})", t.name(), t.key().name());
                    let response = ui.add_sized([40.0, 36.0], egui::Button::selectable(on, "")).on_hover_text(hint);
                    let ink = ui.style().interact_selectable(&response, on).fg_stroke.color;
                    icons::paint(ui.painter(), t, response.rect.shrink(6.0), ink);
                    if response.clicked() {
                        self.pen.tool = t;
                    }
                    if n % 5 == 4 {
                        ui.end_row();
                    }
                }
            });
            if tool == Tool::Crop {
                ui.separator();
                self.crop_controls(ui, actions);
            }
            if tool == Tool::Text {
                ui.separator();
                ui.label(egui::RichText::new("Click the picture, type, then press Enter. Shift+Enter starts a new line.").weak());
            }
            if tool.uses_colour() {
                ui.separator();
                self.colour(ui);
            }
            if tool.uses_size() {
                ui.separator();
                ui.label("Size");
                if ui.add(egui::Slider::new(&mut self.pen.size, 1..=64)).changed() {
                    self.config.size = self.pen.size;
                    self.config.save();
                }
            }
            if tool.has_fill() {
                ui.separator();
                ui.label("Shape");
                for (fill, label) in [(Fill::Outline, "Outline"), (Fill::Filled, "Filled"), (Fill::Both, "Both")] {
                    if ui.radio(self.pen.fill == fill, label).clicked() {
                        self.pen.fill = fill;
                        self.config.fill = fill;
                        self.config.save();
                    }
                }
            }
        });
    }

    /// What Crop has chosen, the shape to hold it to, and the ways to keep it or let it go.
    fn crop_controls(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        ui.label("Crop");
        let size = (self.doc.bitmap.width as i32, self.doc.bitmap.height as i32);
        match self.crop {
            Some(r) => ui.label(format!("{} × {} px", r.width(), r.height())),
            None => ui.label(egui::RichText::new("Drag on the picture to choose what to keep.").weak()),
        };
        ui.add_space(4.0);
        ui.label("Shape");
        let before = (self.aspect, self.portrait);
        ui.horizontal_wrapped(|ui| {
            let custom = matches!(self.aspect, Aspect::Custom(..));
            let choices = [
                (Aspect::Free, "Free"),
                (Aspect::Original, "Original"),
                (Aspect::Fixed(1, 1), "1:1"),
                (Aspect::Fixed(4, 3), "4:3"),
                (Aspect::Fixed(3, 2), "3:2"),
                (Aspect::Fixed(16, 9), "16:9"),
            ];
            for (aspect, label) in choices {
                if ui.add(egui::Button::selectable(self.aspect == aspect, label)).clicked() {
                    self.aspect = aspect;
                }
            }
            if ui.add(egui::Button::selectable(custom, "Custom")).clicked() && !custom {
                self.aspect = Aspect::Custom(4, 5);
            }
        });
        if let Aspect::Custom(w, h) = &mut self.aspect {
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(w).range(1..=99));
                ui.label(":");
                ui.add(egui::DragValue::new(h).range(1..=99));
            });
        }
        if !matches!(self.aspect, Aspect::Free | Aspect::Original | Aspect::Fixed(1, 1)) {
            ui.checkbox(&mut self.portrait, "Portrait");
        }
        // A new shape applies to what's chosen, or chooses something in it.
        if (self.aspect, self.portrait) != before
            && let Some(ratio) = self.aspect.ratio(size, self.portrait)
        {
            self.crop = Some(match self.crop {
                Some(r) => crop::fit_ratio(r, ratio),
                None => crop::starting(ratio, size),
            });
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui.add_enabled(self.crop.is_some(), egui::Button::new("Crop")).on_hover_text("Keep this part (Enter)").clicked() {
                actions.push(Action::ApplyCrop);
            }
            if ui.add_enabled(self.crop.is_some(), egui::Button::new("Clear")).on_hover_text("Choose again (Esc)").clicked() {
                self.crop = None;
            }
        });
    }

    /// The colour the tool paints with, in the picker the skin builder uses. Most tools have one
    /// (the foreground); the eraser paints the background; a shape that's both outline and fill
    /// has two, to choose between.
    fn colour(&mut self, ui: &mut egui::Ui) {
        let tool = self.pen.tool;
        let two = tool.has_fill() && self.pen.fill == Fill::Both;
        let slot = match (tool, two) {
            (Tool::Eraser, _) => Slot::Background,
            (_, true) => self.slot,
            _ => Slot::Foreground,
        };
        if two {
            ui.horizontal(|ui| {
                for (which, label, colour) in
                    [(Slot::Foreground, "Outline", self.pen.fg), (Slot::Background, "Fill", self.pen.bg)]
                {
                    let on = slot == which;
                    let response = ui.add(egui::Button::selectable(on, label));
                    let swatch = Rect::from_min_size(response.rect.left_bottom() - Vec2::new(0.0, 4.0), Vec2::new(response.rect.width(), 4.0));
                    ui.painter().rect_filled(swatch, 0.0, to_color32(colour));
                    if response.clicked() {
                        self.slot = which;
                    }
                }
            });
        } else {
            ui.label(if tool == Tool::Eraser { "Erases to" } else { "Colour" });
        }
        let now = if slot == Slot::Foreground { self.pen.fg } else { self.pen.bg };
        let mut colour = to_color32(now);
        // The picker is as wide as the slider is: the panel takes its width from it.
        ui.spacing_mut().slider_width = 180.0;
        let mut changed =
            egui::color_picker::color_picker_color32(ui, &mut colour, egui::color_picker::Alpha::Opaque);
        let mut next = from_color32(colour);
        // Hex as well as the picker's numbers, which egui offers only as bytes or fractions.
        if let Some(typed) = hex_field(ui, now) {
            next = typed;
            changed = true;
        }
        if changed {
            match slot {
                Slot::Foreground => self.pen.fg = next,
                Slot::Background => self.pen.bg = next,
            }
        }
    }

    /// The picture, zoomed, and the pointer's strokes on it.
    fn canvas(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        // What a tool left half done goes when another is picked: text is written, a crop dropped.
        if self.pen.tool != Tool::Text {
            self.place_text();
        }
        if self.pen.tool != Tool::Crop {
            self.crop = None;
            self.crop_drag = None;
        }
        // The text being typed shows in the picture as it's typed.
        let preview =
            self.typing.as_ref().map(|t| (t.at, t.content.clone(), self.pen.size, self.pen.fg)).filter(|p| !p.1.is_empty());
        if preview != self.preview {
            self.preview = preview;
            self.texture_stale = true;
        }
        // Zoomed out the picture is smoothed, or it would be a scatter of pixels; in, each is sharp.
        let smooth = self.zoom < 1.0;
        if smooth != self.smooth {
            self.smooth = smooth;
            self.texture_stale = true;
        }
        if self.texture_stale || self.texture.is_none() {
            let options = if smooth { TextureOptions::LINEAR } else { TextureOptions::NEAREST };
            let composed;
            let b = match &self.preview {
                Some((at, content, size, colour)) => {
                    let mut with = self.doc.bitmap.clone();
                    text::draw(&mut with, *at, text::pixels(*size), content, *colour);
                    composed = with;
                    &composed
                }
                None => &self.doc.bitmap,
            };
            let image = egui::ColorImage::from_rgba_unmultiplied([b.width, b.height], &b.to_bytes());
            match &mut self.texture {
                Some(texture) => texture.set(image, options),
                None => self.texture = Some(ctx.load_texture("picture", image, options)),
            }
            self.texture_stale = false;
        }
        let picture = Vec2::new(self.doc.bitmap.width as f32, self.doc.bitmap.height as f32);
        let viewport = ui.available_rect_before_wrap();
        self.window = viewport;
        if self.fit {
            let room = viewport.size() - Vec2::splat(24.0);
            self.zoom = (room.x / picture.x).min(room.y / picture.y).clamp(ZOOMS[0], 1.0);
            self.scroll = Vec2::ZERO;
            self.scroll_set = true;
            self.fit = false;
        }
        self.look(&ctx, viewport);

        let size = picture * self.zoom;
        let (zoom, crop) = (self.zoom, self.crop);
        let mut painted = None;
        // The wheel and the touchpad are ours (`look`): the scroll area keeps its bars.
        let mut scroller = egui::ScrollArea::both()
            .auto_shrink(false)
            .scroll_source(egui::scroll_area::ScrollSource::SCROLL_BAR);
        if self.scroll_set {
            scroller = scroller.scroll_offset(self.scroll);
        }
        let output = scroller.show(ui, |ui| {
                // Room around the picture, and it sits in the middle when it's small.
                let area = (size + Vec2::splat(view::MARGIN * 2.0)).max(ui.available_size());
                let (outer, response) = ui.allocate_exact_size(area, Sense::click_and_drag());
                let rect = Rect::from_center_size(outer.center(), size);
                let painter = ui.painter_at(outer);
                let line = ui.visuals().widgets.noninteractive.bg_stroke;
                painter.rect_stroke(rect.expand(1.0), 0.0, line, egui::StrokeKind::Outside);
                if let Some(texture) = &self.texture {
                    painter.image(
                        texture.id(),
                        rect,
                        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }
                if let Some(region) = crop {
                    let kept = region.on_screen(rect.min, zoom);
                    let dim = Color32::from_black_alpha(130);
                    for outside in [
                        Rect::from_min_max(rect.min, Pos2::new(rect.max.x, kept.top())),
                        Rect::from_min_max(Pos2::new(rect.min.x, kept.bottom()), rect.max),
                        Rect::from_min_max(Pos2::new(rect.min.x, kept.top()), Pos2::new(kept.left(), kept.bottom())),
                        Rect::from_min_max(Pos2::new(kept.right(), kept.top()), Pos2::new(rect.max.x, kept.bottom())),
                    ] {
                        painter.rect_filled(outside, 0.0, dim);
                    }
                    painter.rect_stroke(kept, 0.0, egui::Stroke::new(3.0, Color32::from_black_alpha(160)), egui::StrokeKind::Outside);
                    painter.rect_stroke(kept, 0.0, egui::Stroke::new(1.5, Color32::WHITE), egui::StrokeKind::Outside);
                    // Rule-of-thirds lines inside, faint: where to put what matters.
                    let faint = egui::Stroke::new(1.0, Color32::from_white_alpha(70));
                    for i in 1..3 {
                        let (x, y) = (kept.left() + kept.width() * i as f32 / 3.0, kept.top() + kept.height() * i as f32 / 3.0);
                        painter.line_segment([Pos2::new(x, kept.top()), Pos2::new(x, kept.bottom())], faint);
                        painter.line_segment([Pos2::new(kept.left(), y), Pos2::new(kept.right(), y)], faint);
                    }
                    // The handles to take hold of: white with a dark edge, over the picture's edge.
                    let handles = ui.painter().clone();
                    for handle in Handle::ALL {
                        let at = Rect::from_center_size(handle.position(kept), Vec2::splat(HANDLE_SIZE));
                        handles.rect_filled(at, 1.5, Color32::WHITE);
                        handles.rect_stroke(at, 1.5, egui::Stroke::new(1.0, Color32::from_black_alpha(200)), egui::StrokeKind::Outside);
                    }
                }
                painted = Some((rect, response));
            });
        self.scroll = output.state.offset;
        self.scroll_set = false;
        let Some((rect, response)) = painted else { return };

        let to_pixel = |p: Pos2| {
            let at = (p - rect.min) / zoom;
            (at.x.floor() as i32, at.y.floor() as i32)
        };
        let pointer = ctx.input(|i| i.pointer.interact_pos());
        self.cursor = pointer.filter(|p| rect.contains(*p)).map(to_pixel);
        if response.hovered() && self.cursor.is_some() {
            ctx.set_cursor_icon(if self.pen.tool == Tool::Picker {
                egui::CursorIcon::Cell
            } else {
                egui::CursorIcon::Crosshair
            });
        }

        let (pressed, down, shift) = ctx.input(|i| {
            let pressed = if i.pointer.button_pressed(egui::PointerButton::Primary) {
                Some(Button::Primary)
            } else if i.pointer.button_pressed(egui::PointerButton::Secondary) {
                Some(Button::Secondary)
            } else {
                None
            };
            (pressed, i.pointer.primary_down() || i.pointer.secondary_down(), i.modifiers.shift)
        });
        self.typing_box(&ctx, rect);
        if self.panning {
            ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
            return;
        }
        let hovered = response.hovered() || response.is_pointer_button_down_on();
        match self.pen.tool {
            Tool::Crop => {
                self.crop_pointer(&ctx, rect, pointer, pressed == Some(Button::Primary) && hovered, down, to_pixel(pointer.unwrap_or(rect.min)));
                return;
            }
            Tool::Text => {
                if let (Some(Button::Primary), Some(pos), true) = (pressed, self.cursor, hovered) {
                    self.place_text();
                    self.typing = Some(Typing { at: pos, content: String::new(), fresh: true });
                }
                return;
            }
            _ => {}
        }
        if !self.doc.drawing() {
            if let (Some(button), Some(pos), true) =
                (pressed, self.cursor, response.hovered() || response.is_pointer_button_down_on())
            {
                if let Some(colour) = self.doc.begin(&self.pen, button, pos) {
                    match button {
                        Button::Primary => self.pen.fg = colour,
                        Button::Secondary => self.pen.bg = colour,
                    }
                }
                self.texture_stale = true;
            }
        } else if down {
            if let Some(p) = pointer {
                self.doc.drag(&self.pen, to_pixel(p), shift);
                self.texture_stale = true;
            }
        } else {
            self.doc.end();
            self.texture_stale = true;
        }
    }

    /// Moving about the picture: the wheel zooms about the pointer (Ctrl and the wheel too, and a
    /// pinch), Shift and the wheel pan sideways and Alt and the wheel up and down, a touchpad's
    /// swipe pans, and the middle button, or Space and the left one, drags it about.
    fn look(&mut self, ctx: &egui::Context, viewport: Rect) {
        let pointer = ctx.input(|i| i.pointer.hover_pos().or(i.pointer.interact_pos()));
        let over = pointer.is_some_and(|p| viewport.contains(p));
        if over && let Some(p) = pointer {
            let anchor = p - viewport.min;
            for event in ctx.input(|i| i.events.clone()) {
                match event {
                    egui::Event::MouseWheel { unit, delta, modifiers, .. } => {
                        let notched = unit != egui::MouseWheelUnit::Point;
                        match view::gesture(notched, delta, modifiers) {
                            Gesture::Zoom(factor) => self.zoom_at(factor, anchor),
                            Gesture::Pan(by) => {
                                self.scroll += by;
                                self.scroll_set = true;
                            }
                        }
                    }
                    egui::Event::Zoom(factor) => self.zoom_at(factor, anchor),
                    _ => {}
                }
            }
        }
        // Dragging it about, by the hand cursor's two ways.
        let (middle, primary, delta) = ctx.input(|i| (i.pointer.middle_down(), i.pointer.primary_down(), i.pointer.delta()));
        let space = !ctx.egui_wants_keyboard_input() && ctx.input(|i| i.key_down(Key::Space));
        let dragging = middle || (space && primary);
        if dragging && (over || self.panning) {
            self.panning = true;
            self.scroll -= delta;
            self.scroll_set = true;
        } else {
            self.panning = false;
            if over && space {
                ctx.set_cursor_icon(egui::CursorIcon::Grab);
            }
        }
    }

    /// The pointer's part in Crop: pressing on a handle pulls it, inside the selection carries it,
    /// elsewhere draws a new one; held to the chosen shape when there is one.
    fn crop_pointer(
        &mut self,
        ctx: &egui::Context,
        picture: Rect,
        pointer: Option<Pos2>,
        pressed: bool,
        down: bool,
        at: (i32, i32),
    ) {
        let size = (self.doc.bitmap.width as i32, self.doc.bitmap.height as i32);
        let ratio = self.aspect.ratio(size, self.portrait);
        let inside = |p: (i32, i32)| (p.0.clamp(0, size.0 - 1), p.1.clamp(0, size.1 - 1));
        let on_handle = match (self.crop, pointer) {
            (Some(r), Some(p)) => crop::hit(r.on_screen(picture.min, self.zoom), p, HANDLE_GRAB),
            _ => None,
        };
        if self.crop_drag.is_none() {
            // What a press here would do, said by the pointer.
            if let Some(handle) = on_handle {
                ctx.set_cursor_icon(handle.cursor());
            } else if self.crop.is_some_and(|r| r.contains(at)) {
                ctx.set_cursor_icon(egui::CursorIcon::Grab);
            }
            if pressed && pointer.is_some() {
                self.crop_drag = Some(match (self.crop, on_handle) {
                    (Some(r), Some(handle)) => CropDrag::Resize(handle, r),
                    (Some(r), None) if r.contains(at) => CropDrag::Move((at.0 - r.x0, at.1 - r.y0)),
                    _ => {
                        let corner = inside(at);
                        self.crop = Some(Region::between(corner, corner));
                        CropDrag::New(corner)
                    }
                });
            }
            return;
        }
        let Some(drag) = self.crop_drag else { return };
        if !down {
            self.crop_drag = None;
            // A click that went nowhere chooses nothing.
            if matches!(drag, CropDrag::New(_)) && self.crop.is_some_and(|r| r.width() == 1 && r.height() == 1) {
                self.crop = None;
            }
            return;
        }
        match drag {
            CropDrag::New(corner) => {
                self.crop = Some(match ratio {
                    Some(ratio) => crop::from_corner(corner, at, ratio, size),
                    None => Region::between(corner, inside(at)),
                });
            }
            CropDrag::Move(grab) => {
                ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
                if let Some(r) = self.crop {
                    self.crop = Some(crop::moved(r, grab, at, size));
                }
            }
            CropDrag::Resize(handle, start) => {
                ctx.set_cursor_icon(handle.cursor());
                self.crop = Some(match (ratio, crop::opposite_corner(start, handle)) {
                    (Some(ratio), Some(corner)) => crop::from_corner(corner, at, ratio, size),
                    (Some(ratio), None) => crop::from_side(start, handle, at, ratio, size),
                    (None, _) => crop::resize(start, handle, at, size),
                });
            }
        }
    }

    /// Writes the text that's been typed into the picture, if any, and closes its box.
    fn place_text(&mut self) {
        let Some(typing) = self.typing.take() else { return };
        if !typing.content.trim().is_empty() {
            self.doc.text(typing.at, text::pixels(self.pen.size), &typing.content, self.pen.fg);
        }
        self.preview = None;
        self.texture_stale = true;
    }

    /// The box text is typed in, over the picture a little above where it will go: Enter writes it,
    /// Esc drops it, Shift+Enter starts a line.
    fn typing_box(&mut self, ctx: &egui::Context, picture: Rect) {
        let Some(typing) = &mut self.typing else { return };
        let at = picture.min + Vec2::new(typing.at.0 as f32, typing.at.1 as f32) * self.zoom;
        let pos = Pos2::new(at.x, (at.y - 44.0).max(picture.top()));
        let (mut enter, mut escape) = (false, false);
        egui::Area::new(egui::Id::new("typing")).order(egui::Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_width(260.0);
                if ui.input(|i| i.key_pressed(Key::Escape)) {
                    escape = true;
                }
                if ui.input(|i| i.key_pressed(Key::Enter) && !i.modifiers.shift) {
                    ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter));
                    enter = true;
                }
                let edit = ui.add(
                    egui::TextEdit::multiline(&mut typing.content)
                        .desired_rows(1)
                        .desired_width(f32::INFINITY)
                        .hint_text("Type, then Enter"),
                );
                if typing.fresh {
                    edit.request_focus();
                    typing.fresh = false;
                }
            });
        });
        if enter {
            self.place_text();
        } else if escape {
            self.typing = None;
            self.preview = None;
            self.texture_stale = true;
        }
    }

    fn status_bar(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let b = &self.doc.bitmap;
            match self.cursor {
                Some((x, y)) => ui.label(format!("{x}, {y}")),
                None => ui.label(""),
            };
            ui.separator();
            ui.label(format!("{} × {}", b.width, b.height));
            ui.separator();
            ui.label(format!("{:.0}%", self.zoom * 100.0));
            if let Some(status) = &self.status {
                ui.separator();
                ui.label(status);
            }
        });
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.dialog else { return };
        let mut outcome = None; // Some(true): accepted, Some(false): cancelled
        let mut discard = false;
        let title = match dialog {
            Dialog::New { .. } => "New Image",
            Dialog::Resize { .. } => "Canvas Size",
            Dialog::Unsaved(_) => "Unsaved Changes",
        };
        egui::Window::new(title)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                match dialog {
                    Dialog::New { width, height } | Dialog::Resize { width, height } => {
                        egui::Grid::new("size").show(ui, |ui| {
                            ui.label("Width");
                            ui.add(egui::DragValue::new(width).range(1..=16384).suffix(" px"));
                            ui.end_row();
                            ui.label("Height");
                            ui.add(egui::DragValue::new(height).range(1..=16384).suffix(" px"));
                            ui.end_row();
                        });
                    }
                    Dialog::Unsaved(_) => {
                        ui.label("The picture has changes that aren't saved.");
                    }
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| match dialog {
                    Dialog::Unsaved(_) => {
                        if ui.button("Save").clicked() {
                            outcome = Some(true);
                        }
                        if ui.button("Discard").clicked() {
                            discard = true;
                            outcome = Some(true);
                        }
                        if ui.button("Cancel").clicked() {
                            outcome = Some(false);
                        }
                    }
                    _ => {
                        if ui.button("OK").clicked() || ui.input(|i| i.key_pressed(Key::Enter)) {
                            outcome = Some(true);
                        }
                        if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
                            outcome = Some(false);
                        }
                    }
                });
            });
        let Some(accepted) = outcome else { return };
        let dialog = self.dialog.take();
        if !accepted {
            return;
        }
        match dialog {
            Some(Dialog::New { width, height }) => self.proceed(Then::New(width, height), ctx, false),
            Some(Dialog::Resize { width, height }) => {
                let bg = self.pen.bg;
                self.edit(|b| b.resize_canvas(width, height, bg));
            }
            Some(Dialog::Unsaved(then)) if discard => {
                self.doc.modified = false;
                let pick = matches!(then, Then::New(..));
                self.proceed(then, ctx, pick);
            }
            Some(Dialog::Unsaved(then)) => {
                self.after_save = Some(then);
                self.save(ctx, false);
            }
            None => {}
        }
    }

    fn screenshot(&mut self, ctx: &egui::Context) {
        let Some((path, requested)) = &mut self.screenshot else { return };
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
        ctx.request_repaint();
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        if !self.config.system_title_bar {
            patina::frame::shadow::request(frame);
        }
        let ctx = ui.ctx().clone();
        self.files(&ctx);
        self.screenshot(&ctx);

        // Closing the window asks about unsaved changes first.
        if ctx.input(|i| i.viewport().close_requested())
            && self.doc.modified
            && !self.closing
            && self.screenshot.is_none()
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.dialog = Some(Dialog::Unsaved(Then::Quit));
        }

        let custom = !self.config.system_title_bar;
        // Every frame: Default's colours follow the desktop's light/dark setting.
        let style = self.skin().frame_style(ui.visuals());
        let radius = if custom { frame::corner_radius(&ctx, style.radius) } else { 0 };
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(self.title()));

        let (mut actions, mut right) = (Vec::new(), Vec::new());
        self.shortcuts(&ctx, &mut actions);
        let title = self.title();
        egui::Panel::top("header").frame(frame::header_frame(ui.style(), &style, radius)).show(ui, |ui| {
            TitleBar::new(&title, &style).custom(custom).menu_bar(true).span(frame::header_span(ui)).show(
                ui,
                |ui| self.menus(ui, &mut actions),
                |ui| {
                    // What Draw makes is the picture: saving it is the main action, and Save As
                    // keeps the original beside a new version.
                    if ui.button("Save").on_hover_text("Save the picture (Ctrl+S)").clicked() {
                        right.push(Action::Save);
                    }
                    if ui.button("Save As").on_hover_text("Save a new version (Ctrl+Shift+S)").clicked() {
                        right.push(Action::SaveAs);
                    }
                },
            );
        });

        // The status bar holds both bottom corners.
        egui::Panel::bottom("status")
            .frame(egui::Frame::side_top_panel(ui.style()).corner_radius(egui::CornerRadius {
                sw: radius,
                se: radius,
                ..Default::default()
            }))
            .show(ui, |ui| self.status_bar(ui));
        egui::Panel::left("tools").resizable(false).exact_size(256.0).show(ui, |ui| self.toolbox(ui, &mut actions));
        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(ui.style()).inner_margin(0.0))
            .show(ui, |ui| self.canvas(ui));

        for action in actions.into_iter().chain(right) {
            self.act(&ctx, action);
        }
        self.dialogs(&ctx);

        if custom {
            frame::edges(&ctx, radius, style.border);
        }
    }

    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        frame::clear_color(visuals, !self.config.system_title_bar)
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        frame::release_grab(ctx, raw_input);
    }
}

fn item(ui: &mut egui::Ui, actions: &mut Vec<Action>, label: &str, keys: &str, enabled: bool, action: Action) {
    if ui.add_enabled(enabled, egui::Button::new(label).shortcut_text(keys)).clicked() {
        actions.push(action);
        ui.close();
    }
}

/// A field for the colour as hex. It shows `colour` until it's clicked into; what's typed is taken
/// as soon as it's a colour (`#RRGGBB` or `RGB`), and a half-typed one changes nothing.
fn hex_field(ui: &mut egui::Ui, colour: Rgba) -> Option<Rgba> {
    let id = egui::Id::new("draw-hex");
    let mut taken = None;
    ui.horizontal(|ui| {
        ui.label("Hex");
        let editing = ui.memory(|m| m.has_focus(id));
        let mut text = if editing {
            ui.data(|d| d.get_temp::<String>(id)).unwrap_or_else(|| crate::colour::hex(colour))
        } else {
            crate::colour::hex(colour)
        };
        let response = ui.add(egui::TextEdit::singleline(&mut text).id(id).desired_width(90.0).hint_text("#RRGGBB"));
        if response.changed() {
            taken = crate::colour::parse_hex(&text).filter(|c| *c != colour);
        }
        if response.has_focus() {
            ui.data_mut(|d| d.insert_temp(id, text));
        } else {
            ui.data_mut(|d| d.remove_temp::<String>(id));
        }
    });
    taken
}

fn to_color32(c: Rgba) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}

fn from_color32(c: Color32) -> Rgba {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    [r, g, b, a]
}

/// A little picture for the screenshots, drawn with the tools themselves.
fn demo() -> Bitmap {
    let mut b = Bitmap::new(NEW_SIZE.0, NEW_SIZE.1, WHITE);
    let sky = [150, 205, 250, 255];
    b.rectangle((0, 0), (799, 380), 1, Fill::Filled, sky, sky);
    let grass = [90, 170, 80, 255];
    b.rectangle((0, 380), (799, 599), 1, Fill::Filled, grass, grass);
    b.ellipse((600, 50), (720, 170), 1, Fill::Filled, [255, 210, 40, 255], [255, 210, 40, 255]);
    b.rectangle((220, 250), (460, 430), 6, Fill::Both, [60, 40, 30, 255], [225, 90, 70, 255]);
    b.rectangle((320, 330), (380, 430), 4, Fill::Both, [60, 40, 30, 255], [120, 80, 50, 255]);
    b.line((200, 255), (340, 150), 8, [60, 40, 30, 255]);
    b.line((340, 150), (480, 255), 8, [60, 40, 30, 255]);
    b.ellipse((250, 280), (300, 330), 4, Fill::Both, [60, 40, 30, 255], [250, 240, 160, 255]);
    b.line((60, 520), (180, 470), 5, [40, 90, 40, 255]);
    b.line((180, 470), (320, 520), 5, [40, 90, 40, 255]);
    b.line((320, 520), (470, 470), 5, [40, 90, 40, 255]);
    b.line((470, 470), (700, 530), 5, [40, 90, 40, 255]);
    b
}
