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

const APP: &str = "draw";

/// The skin used until the user picks one: stock egui, following the desktop.
const HOME_SKIN: &str = "Default";

/// Zoom steps, as a fraction of the picture's own pixels.
const ZOOMS: [f32; 12] = [0.125, 0.25, 0.5, 1.0, 2.0, 3.0, 4.0, 6.0, 8.0, 12.0, 16.0, 32.0];

const NEW_SIZE: (usize, usize) = (800, 600);

/// Black and white, then colours as in Paint's palette.
const PALETTE: [Rgba; 28] = [
    [0, 0, 0, 255],
    [128, 128, 128, 255],
    [128, 0, 0, 255],
    [128, 128, 0, 255],
    [0, 128, 0, 255],
    [0, 128, 128, 255],
    [0, 0, 128, 255],
    [128, 0, 128, 255],
    [128, 128, 64, 255],
    [0, 64, 64, 255],
    [0, 128, 255, 255],
    [0, 64, 128, 255],
    [64, 0, 255, 255],
    [128, 64, 0, 255],
    [255, 255, 255, 255],
    [192, 192, 192, 255],
    [255, 0, 0, 255],
    [255, 255, 0, 255],
    [0, 255, 0, 255],
    [0, 255, 255, 255],
    [0, 0, 255, 255],
    [255, 0, 255, 255],
    [255, 255, 128, 255],
    [0, 255, 128, 255],
    [128, 255, 255, 255],
    [128, 128, 255, 255],
    [255, 0, 128, 255],
    [255, 128, 64, 255],
];

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
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, config: Config) -> Self {
        patina::appearance::watch(&cc.egui_ctx);
        let dirs: Vec<_> = skin::app_dir(APP).into_iter().collect();
        let (skins, skin_errors) = skin::load_all(skin::BUILT_IN, &dirs);
        let (io_tx, io_rx) = channel();
        let screenshot = std::env::var_os("DRAW_SCREENSHOT").map(|p| (PathBuf::from(p), false));
        let pen = Pen { tool: Tool::Brush, size: config.size, fill: config.fill, fg: PALETTE[0], bg: WHITE };
        let mut app = Self {
            config,
            skins,
            skin_errors,
            forced_skin: std::env::var("DRAW_SKIN").ok(),
            doc: Document::new(Bitmap::new(NEW_SIZE.0, NEW_SIZE.1, WHITE)),
            pen,
            zoom: 1.0,
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
        };
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
            Action::ActualSize => self.zoom = 1.0,
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

    fn zoom_by(&mut self, steps: i32) {
        let at = ZOOMS.iter().position(|z| *z >= self.zoom - 0.001).unwrap_or(3) as i32;
        self.zoom = ZOOMS[(at + steps).clamp(0, ZOOMS.len() as i32 - 1) as usize];
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
        let suggest = self.doc.path.as_deref().map(file::name).unwrap_or_else(|| "Untitled.png".into());
        file::save(ctx, self.io_tx.clone(), self.doc.bitmap.clone(), path, &suggest);
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
            if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
                self.doc.cancel();
                self.texture_stale = true;
            }
        }
    }

    fn toolbox(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        egui::Grid::new("tools").spacing([4.0, 4.0]).show(ui, |ui| {
            for (n, tool) in Tool::ALL.into_iter().enumerate() {
                let label = tool.name();
                let on = self.pen.tool == tool;
                let hint = format!("{label} ({})", tool.key().name());
                if ui.add_sized([72.0, 24.0], egui::Button::selectable(on, label)).on_hover_text(hint).clicked() {
                    self.pen.tool = tool;
                }
                if n % 2 == 1 {
                    ui.end_row();
                }
            }
        });
        ui.separator();
        let sized = matches!(self.pen.tool, Tool::Brush | Tool::Eraser) || self.pen.tool.is_shape();
        ui.add_enabled_ui(sized, |ui| {
            ui.label("Size");
            if ui.add(egui::Slider::new(&mut self.pen.size, 1..=64)).changed() {
                self.config.size = self.pen.size;
                self.config.save();
            }
        });
        ui.add_enabled_ui(matches!(self.pen.tool, Tool::Rectangle | Tool::Ellipse), |ui| {
            ui.label("Shape");
            for (fill, label) in [(Fill::Outline, "Outline"), (Fill::Filled, "Filled"), (Fill::Both, "Both")] {
                if ui.radio(self.pen.fill == fill, label).clicked() {
                    self.pen.fill = fill;
                    self.config.fill = fill;
                    self.config.save();
                }
            }
        });
    }

    fn palette(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            // The colours in use, as in Paint: foreground over background.
            let mut fg = to_color32(self.pen.fg);
            let mut bg = to_color32(self.pen.bg);
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    if ui.color_edit_button_srgba(&mut fg).on_hover_text("Foreground: left button").changed() {
                        self.pen.fg = from_color32(fg);
                    }
                    if ui.color_edit_button_srgba(&mut bg).on_hover_text("Background: right button").changed() {
                        self.pen.bg = from_color32(bg);
                    }
                });
            });
            ui.separator();
            let swatch = Vec2::splat(18.0);
            egui::Grid::new("palette").spacing([2.0, 2.0]).show(ui, |ui| {
                for (n, colour) in PALETTE.iter().enumerate() {
                    let (rect, response) = ui.allocate_exact_size(swatch, Sense::click());
                    let c = to_color32(*colour);
                    ui.painter().rect_filled(rect, 2.0, c);
                    let outline = ui.visuals().widgets.noninteractive.bg_stroke;
                    ui.painter().rect_stroke(rect, 2.0, outline, egui::StrokeKind::Inside);
                    if response.clicked() {
                        self.pen.fg = *colour;
                    }
                    if response.secondary_clicked() {
                        self.pen.bg = *colour;
                    }
                    if n % 14 == 13 {
                        ui.end_row();
                    }
                }
            });
        });
    }

    /// The picture, zoomed, and the pointer's strokes on it.
    fn canvas(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if self.texture_stale || self.texture.is_none() {
            let b = &self.doc.bitmap;
            let image = egui::ColorImage::from_rgba_unmultiplied([b.width, b.height], &b.to_bytes());
            match &mut self.texture {
                Some(texture) => texture.set(image, TextureOptions::NEAREST),
                None => self.texture = Some(ctx.load_texture("picture", image, TextureOptions::NEAREST)),
            }
            self.texture_stale = false;
        }
        let picture = Vec2::new(self.doc.bitmap.width as f32, self.doc.bitmap.height as f32);
        if self.fit {
            let room = ui.available_size() - Vec2::splat(24.0);
            self.zoom = (room.x / picture.x).min(room.y / picture.y).clamp(ZOOMS[0], 1.0);
            self.fit = false;
        }
        // Ctrl and the wheel zoom, in steps; the plain wheel scrolls.
        let pinch = ctx.input(|i| i.zoom_delta());
        if pinch != 1.0 && ui.rect_contains_pointer(ui.max_rect()) {
            self.zoom_by(if pinch > 1.0 { 1 } else { -1 });
        }

        let size = picture * self.zoom;
        let mut painted = None;
        egui::ScrollArea::both()
            .auto_shrink(false)
            .scroll_source(egui::scroll_area::ScrollSource::SCROLL_BAR | egui::scroll_area::ScrollSource::MOUSE_WHEEL)
            .show(ui, |ui| {
                // Room around the picture, and it sits in the middle when it's small.
                let margin = 12.0;
                let area = (size + Vec2::splat(margin * 2.0)).max(ui.available_size());
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
                painted = Some((rect, response));
            });
        let Some((rect, response)) = painted else { return };

        let to_pixel = |p: Pos2| {
            let at = (p - rect.min) / self.zoom;
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
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
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
                    // What Draw makes is the picture: saving it is the main action.
                    if ui.button("Save").on_hover_text("Save the picture (Ctrl+S)").clicked() {
                        right.push(Action::Save);
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
        egui::Panel::bottom("palette").show(ui, |ui| self.palette(ui));
        egui::Panel::left("tools").resizable(false).show(ui, |ui| self.toolbox(ui));
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
