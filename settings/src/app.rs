//! The window: Patina's frame and title bar, a sidebar of pages, the page,
//! and the status bar (Patina's ADR-0016). The Appearance and Wallpaper
//! pages are here; Night Light's is in `night_light.rs`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, SystemTime};

use egui::{Color32, Rect, RichText, Sense, Stroke, TextureHandle, Ui, vec2};
use patina::frame::{self, TitleBar};
use patina::skin::{self, Skin};

use crate::night_light::NightLightPage;
use crate::wallpapers::{self, Wallpaper};

/// The shell's skins folder, the bar's, where its own skins go.
const SHELL: &str = "patina-bar";
/// How often the bar's config is looked at, to follow a skin chosen in the
/// desktop's menu.
const SKIN_POLL: Duration = Duration::from_secs(1);
/// A wallpaper's tile, in points, and its name under it.
const TILE: egui::Vec2 = vec2(176.0, 110.0);
const TILE_LABEL: f32 = 20.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Appearance,
    Bar,
    Wallpaper,
    Keyboard,
    Dictation,
    NightLight,
    Power,
    Processes,
}

impl Page {
    const ALL: [Page; 8] = [
        Page::Appearance,
        Page::Bar,
        Page::Wallpaper,
        Page::Keyboard,
        Page::Dictation,
        Page::NightLight,
        Page::Power,
        Page::Processes,
    ];

    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "appearance" => Some(Page::Appearance),
            "bar" => Some(Page::Bar),
            "wallpaper" => Some(Page::Wallpaper),
            "keyboard" => Some(Page::Keyboard),
            "dictation" => Some(Page::Dictation),
            "night-light" => Some(Page::NightLight),
            "power" => Some(Page::Power),
            "processes" => Some(Page::Processes),
            _ => None,
        }
    }

    fn title(self) -> &'static str {
        match self {
            Page::Appearance => "Appearance",
            Page::Bar => "Bar",
            Page::Wallpaper => "Wallpaper",
            Page::Keyboard => "Keyboard",
            Page::Dictation => "Dictation",
            Page::NightLight => "Night Light",
            Page::Power => "Power",
            Page::Processes => "Processes",
        }
    }
}

/// The status bar's latest message.
pub struct Message {
    text: String,
    error: bool,
}

impl Message {
    pub fn note(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            error: false,
        }
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            error: true,
        }
    }
}

pub struct App {
    custom_frame: bool,
    page: Page,
    skins: Vec<Skin>,
    /// Skins that failed to load, shown on the status bar's hover.
    skin_errors: Vec<String>,
    /// The shell's skin, by index into `skins`.
    skin: usize,
    /// The bar's config's modification time, to follow changes to it.
    bar_mtime: Option<SystemTime>,
    wallpapers: Vec<Wallpaper>,
    /// The wallpaper in use; `None` for the default.
    wallpaper: Option<PathBuf>,
    /// Each wallpaper's thumbnail, once decoded; `None` if it won't.
    thumbnails: HashMap<PathBuf, Option<TextureHandle>>,
    decoded: Option<mpsc::Receiver<(PathBuf, Option<egui::ColorImage>)>>,
    /// The flask, for the default wallpaper's tile.
    flask: Option<TextureHandle>,
    bar: crate::bar::BarPage,
    gaps: crate::gaps::GapsSection,
    icons: crate::icons::IconsSection,
    keyboard: crate::keyboard::KeyboardPage,
    dictation: crate::dictation::DictationPage,
    night_light: NightLightPage,
    power: crate::power::PowerPage,
    processes: crate::processes::ProcessesPage,
    message: Option<Message>,
}

impl App {
    pub fn new(ctx: &egui::Context, page: Page) -> Self {
        patina::appearance::watch(ctx);
        let dirs: Vec<_> = skin::app_dir(SHELL).into_iter().collect();
        let (skins, skin_errors) = skin::load_all(skin::BUILT_IN, &dirs);
        let mut app = Self {
            custom_frame: true,
            page,
            skins,
            skin_errors,
            skin: 0,
            bar_mtime: None,
            wallpapers: Vec::new(),
            wallpaper: patina_shell::bar::wallpaper(),
            thumbnails: HashMap::new(),
            decoded: None,
            flask: None,
            bar: crate::bar::BarPage::new(),
            gaps: crate::gaps::GapsSection::new(),
            icons: crate::icons::IconsSection::new(),
            keyboard: crate::keyboard::KeyboardPage::new(ctx),
            dictation: crate::dictation::DictationPage::new(),
            night_light: NightLightPage::new(ctx),
            power: crate::power::PowerPage::new(ctx),
            processes: crate::processes::ProcessesPage::new(ctx),
            message: None,
        };
        app.follow_shell_skin(ctx);
        app
    }

    /// Takes the shell's skin from the bar's config if it changed, as it
    /// does when it's chosen in the desktop's menu.
    fn follow_shell_skin(&mut self, ctx: &egui::Context) {
        let mtime = patina_shell::bar::config_path()
            .and_then(|p| std::fs::metadata(p).ok())
            .and_then(|m| m.modified().ok());
        if mtime == self.bar_mtime && self.bar_mtime.is_some() {
            return;
        }
        self.bar_mtime = mtime;
        let name = patina_shell::bar::skin().unwrap_or_else(|| skin::DEFAULT_SKIN.to_string());
        self.skin = self.skins.iter().position(|s| s.name == name).unwrap_or(0);
        self.skins[self.skin].apply(ctx);
        self.wallpaper = patina_shell::bar::wallpaper();
        self.bar.reload();
    }

    fn choose_skin(&mut self, index: usize, ctx: &egui::Context) {
        match patina_shell::bar::set("skin", &self.skins[index].name) {
            Ok(()) => {
                self.skin = index;
                self.skins[index].apply(ctx);
            }
            Err(e) => self.message = Some(Message::error(format!("Couldn't save the skin: {e}"))),
        }
    }

    fn choose_wallpaper(&mut self, path: Option<PathBuf>) {
        let value = path
            .as_deref()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        match patina_shell::bar::set("wallpaper", &value) {
            Ok(()) => self.wallpaper = path,
            Err(e) => {
                self.message = Some(Message::error(format!("Couldn't save the wallpaper: {e}")))
            }
        }
    }

    fn open(&mut self, path: PathBuf) {
        if let Err(e) = patina::open::path(&path) {
            self.message = Some(Message::error(format!(
                "Couldn't open {}: {e}",
                path.display()
            )));
        }
    }

    fn menus(&mut self, ui: &mut Ui) {
        ui.menu_button("File", |ui| {
            if ui
                .add(egui::Button::new("Quit").shortcut_text("Ctrl+Q"))
                .clicked()
            {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
        ui.menu_button("View", |ui| {
            for page in Page::ALL {
                if ui.radio(self.page == page, page.title()).clicked() {
                    self.page = page;
                    ui.close();
                }
            }
            ui.separator();
            let mut system = !self.custom_frame;
            if ui.checkbox(&mut system, "System title bar").changed() {
                self.custom_frame = !system;
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Decorations(system));
            }
        });
    }

    fn sidebar(&mut self, ui: &mut Ui) {
        ui.add_space(6.0);
        for page in Page::ALL {
            let selected = self.page == page;
            if ui
                .add(
                    egui::Button::selectable(selected, page.title())
                        .min_size(vec2(ui.available_width(), 28.0)),
                )
                .clicked()
            {
                self.page = page;
            }
        }
    }

    fn status_bar(&self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let weak = ui.visuals().weak_text_color();
            ui.label(
                RichText::new(concat!("v", env!("CARGO_PKG_VERSION")))
                    .small()
                    .color(weak),
            );
            ui.separator();
            if let Some(m) = &self.message {
                let colour = if m.error {
                    ui.visuals().error_fg_color
                } else {
                    weak
                };
                ui.label(RichText::new(&m.text).small().color(colour));
            } else if !self.skin_errors.is_empty() {
                ui.label(
                    RichText::new(format!("{} skins didn't load", self.skin_errors.len()))
                        .small()
                        .color(ui.visuals().warn_fg_color),
                )
                .on_hover_text(self.skin_errors.join("\n"));
            }
        });
    }

    /// The shell's skin, and its skins folder, as the desktop's menu
    /// offers them; the gaps; and the icon theme.
    fn appearance(&mut self, ui: &mut Ui) {
        ui.heading("Appearance");
        ui.label("The skin the bar, the menus, the lock screen and the wallpaper wear.");
        ui.add_space(8.0);
        let mut chosen = None;
        for (i, skin) in self.skins.iter().enumerate() {
            let name = match &skin.author {
                Some(author) if skin.path.is_some() => format!("{} — {author}", skin.name),
                _ => skin.name.clone(),
            };
            let response = ui.radio(i == self.skin, name);
            let response = match (&skin.path, &skin.author) {
                (Some(path), _) => response.on_hover_text(path.display().to_string()),
                (None, Some(author)) => response.on_hover_text(format!("Built in, by {author}")),
                (None, None) => response.on_hover_text("Stock egui"),
            };
            if response.clicked() {
                chosen = Some(i);
            }
        }
        if let Some(i) = chosen {
            self.choose_skin(i, ui.ctx());
        }
        ui.add_space(12.0);
        if let Some(dir) = skin::app_dir(SHELL) {
            self.skins_folder(ui, dir);
        }
        ui.add_space(16.0);
        self.gaps.ui(ui, &mut self.message);
        ui.add_space(16.0);
        self.icons.ui(ui, &mut self.message);
    }

    /// Copying the built-in skins to the skins folder, and opening it.
    fn skins_folder(&mut self, ui: &mut Ui, dir: PathBuf) {
        ui.horizontal(|ui| {
            if ui
                .button("Copy built-in skins to folder")
                .on_hover_text(dir.display().to_string())
                .clicked()
            {
                self.message = Some(match skin::export_built_ins(&dir, skin::BUILT_IN) {
                    Ok(export) if export.copied.is_empty() => {
                        Message::note("The built-in skins are already in the skins folder")
                    }
                    Ok(export) => Message::note(format!(
                        "Copied {} to the skins folder",
                        export.copied.join(", ")
                    )),
                    Err(e) => Message::error(format!("Couldn't copy skins: {e}")),
                });
            }
            if ui.button("Open skins folder").clicked() {
                let _ = std::fs::create_dir_all(&dir);
                self.open(dir.clone());
            }
        });
    }

    /// The default and the images on offer, as tiles.
    fn wallpaper_page(&mut self, ui: &mut Ui) {
        if self.decoded.is_none() {
            self.wallpapers = wallpapers::find();
            let paths = self.wallpapers.iter().map(|w| w.path.clone()).collect();
            self.decoded = Some(wallpapers::thumbnails(paths, ui.ctx().clone()));
        }
        if let Some(decoded) = &self.decoded {
            while let Ok((path, image)) = decoded.try_recv() {
                let texture = image.map(|i| {
                    ui.ctx()
                        .load_texture(path.display().to_string(), i, Default::default())
                });
                self.thumbnails.insert(path, texture);
            }
        }
        ui.horizontal(|ui| {
            ui.heading("Wallpaper");
            if let Some(dir) = wallpapers::user_dir() {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .button("Open wallpapers folder")
                        .on_hover_text(dir.display().to_string())
                        .clicked()
                    {
                        let _ = std::fs::create_dir_all(&dir);
                        self.open(dir);
                    }
                });
            }
        });
        ui.label("Your own go in Pictures/Wallpapers.");
        ui.add_space(8.0);

        let mut chosen = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(12.0, 12.0);
                let flask = self.flask(ui.ctx());
                let response = tile(ui, self.wallpaper.is_none(), "Sector F Labs", |ui, rect| {
                    default_wallpaper(ui, rect, flask.as_ref())
                })
                .on_hover_text("The default, in the skin's colours");
                if response.clicked() {
                    chosen = Some(None);
                }
                for w in &self.wallpapers {
                    let on = self.wallpaper.as_deref() == Some(w.path.as_path());
                    let thumbnail = self.thumbnails.get(&w.path);
                    let response = tile(ui, on, &w.name, |ui, rect| match thumbnail {
                        Some(Some(texture)) => fill(ui, rect, texture),
                        Some(None) => {
                            ui.painter()
                                .rect_filled(rect, 4.0, ui.visuals().faint_bg_color);
                        }
                        None => {
                            ui.painter()
                                .rect_filled(rect, 4.0, ui.visuals().faint_bg_color);
                            ui.put(rect, egui::Spinner::new());
                        }
                    })
                    .on_hover_text(w.path.display().to_string());
                    if response.clicked() {
                        chosen = Some(Some(w.path.clone()));
                    }
                }
            });
        });
        if let Some(path) = chosen {
            self.choose_wallpaper(path);
        }
    }

    fn flask(&mut self, ctx: &egui::Context) -> Option<TextureHandle> {
        if self.flask.is_none() {
            self.flask = patina_system::logo::rasterise(128)
                .map(|i| ctx.load_texture("flask", i, Default::default()));
        }
        self.flask.clone()
    }
}

/// A wallpaper's tile: its picture, drawn by `draw`, with its name under
/// it, outlined when it's the one in use.
fn tile(
    ui: &mut Ui,
    selected: bool,
    name: &str,
    draw: impl FnOnce(&mut Ui, Rect),
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(TILE + vec2(0.0, TILE_LABEL), Sense::click());
    let picture = Rect::from_min_size(rect.min, TILE);
    draw(ui, picture);
    let v = ui.visuals();
    let stroke = if selected {
        Stroke::new(3.0, v.selection.bg_fill)
    } else if response.hovered() {
        v.widgets.hovered.bg_stroke
    } else {
        v.widgets.noninteractive.bg_stroke
    };
    ui.painter()
        .rect_stroke(picture, 4.0, stroke, egui::StrokeKind::Outside);
    let label = Rect::from_min_size(picture.left_bottom(), vec2(TILE.x, TILE_LABEL));
    let text = egui::WidgetText::from(RichText::new(name).small()).into_galley(
        ui,
        Some(egui::TextWrapMode::Truncate),
        TILE.x,
        egui::TextStyle::Small,
    );
    let at = egui::pos2(
        label.center().x - text.size().x / 2.0,
        label.center().y - text.size().y / 2.0,
    );
    ui.painter().galley(at, text, v.text_color());
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// `texture` filling `rect`, cropped to its shape, as the wallpaper fills
/// the screen.
fn fill(ui: &Ui, rect: Rect, texture: &TextureHandle) {
    let [w, h] = texture.size().map(|s| s as f32);
    let (image, tile) = (w / h, rect.width() / rect.height());
    let uv = if image > tile {
        let keep = tile / image;
        Rect::from_min_max(
            egui::pos2((1.0 - keep) / 2.0, 0.0),
            egui::pos2((1.0 + keep) / 2.0, 1.0),
        )
    } else {
        let keep = image / tile;
        Rect::from_min_max(
            egui::pos2(0.0, (1.0 - keep) / 2.0),
            egui::pos2(1.0, (1.0 + keep) / 2.0),
        )
    };
    egui::Image::from_texture(texture)
        .uv(uv)
        .corner_radius(4.0)
        .paint_at(ui, rect);
}

/// The default wallpaper, small: the flask on the panel colour, tinted as
/// patina-wallpaper tints it.
fn default_wallpaper(ui: &Ui, rect: Rect, flask: Option<&TextureHandle>) {
    let v = ui.visuals();
    ui.painter().rect_filled(rect, 4.0, v.panel_fill);
    if let Some(flask) = flask {
        let side = rect.height() * 0.6;
        let at = Rect::from_center_size(rect.center(), vec2(side, side));
        let tint: Color32 = v.selection.bg_fill;
        egui::Image::from_texture(flask).tint(tint).paint_at(ui, at);
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.draw(ui);
    }

    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        frame::clear_color(visuals, self.custom_frame)
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        frame::release_grab(ctx, raw_input);
    }
}

impl App {
    fn draw(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        self.follow_shell_skin(&ctx);
        ctx.request_repaint_after(SKIN_POLL);
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Q)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        // Every frame: Default's colours follow the desktop.
        let style = self.skins[self.skin].frame_style(ui.visuals());
        let radius = if self.custom_frame {
            frame::corner_radius(&ctx, style.radius)
        } else {
            0
        };

        egui::Panel::top("header")
            .frame(frame::header_frame(ui.style(), &style, radius))
            .show(ui, |ui| {
                TitleBar::new("Settings", &style)
                    .custom(self.custom_frame)
                    .menu_bar(true)
                    .show(ui, |ui| self.menus(ui), |_| {});
            });
        // Across the bottom, before the sidebar, holding both corners.
        egui::Panel::bottom("status")
            .frame(
                egui::Frame::side_top_panel(ui.style())
                    .inner_margin(egui::Margin::symmetric(10, 4))
                    .corner_radius(egui::CornerRadius {
                        sw: radius,
                        se: radius,
                        ..Default::default()
                    }),
            )
            .show(ui, |ui| self.status_bar(ui));
        egui::Panel::left("pages")
            .frame(egui::Frame::side_top_panel(ui.style()))
            .default_size(180.0)
            .resizable(false)
            .show(ui, |ui| self.sidebar(ui));
        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(ui.style()).inner_margin(20))
            .show(ui, |ui| match self.page {
                Page::Appearance => {
                    egui::ScrollArea::vertical().show(ui, |ui| self.appearance(ui));
                }
                Page::Bar => {
                    egui::ScrollArea::vertical().show(ui, |ui| self.bar.ui(ui, &mut self.message));
                }
                Page::Wallpaper => self.wallpaper_page(ui),
                Page::Keyboard => {
                    egui::ScrollArea::vertical()
                        .show(ui, |ui| self.keyboard.ui(ui, &mut self.message));
                }
                Page::Dictation => {
                    egui::ScrollArea::vertical()
                        .show(ui, |ui| self.dictation.ui(ui, &mut self.message));
                }
                Page::NightLight => {
                    egui::ScrollArea::vertical()
                        .show(ui, |ui| self.night_light.ui(ui, &mut self.message));
                }
                Page::Power => {
                    egui::ScrollArea::vertical()
                        .show(ui, |ui| self.power.ui(ui, &mut self.message));
                }
                Page::Processes => {
                    egui::ScrollArea::vertical()
                        .show(ui, |ui| self.processes.ui(ui, &mut self.message));
                }
            });

        if self.custom_frame {
            frame::edges(&ctx, radius, style.border);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Draws each page in software, as PNGs in the folder
    /// PATINA_SETTINGS_PREVIEW names, to look at. It reads the real
    /// configs, and writes nothing.
    #[test]
    #[ignore]
    fn preview_the_pages() {
        let dir = PathBuf::from(std::env::var("PATINA_SETTINGS_PREVIEW").unwrap());
        let (w, h) = (860usize, 600usize);
        for page in Page::ALL {
            let ctx = egui::Context::default();
            let mut app = App::new(&ctx, page);
            let mut painter = patina_shell::raster::Painter::default();
            let mut pixels = vec![0u32; w * h];
            // Fonts load on the first pass, thumbnails and D-Bus over a few.
            for _ in 0..30 {
                let input = egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::pos2(0.0, 0.0),
                        vec2(w as f32, h as f32),
                    )),
                    max_texture_side: Some(8192),
                    ..Default::default()
                };
                let out = ctx.run_ui(input, |ui| app.draw(ui));
                let primitives = ctx.tessellate(out.shapes, out.pixels_per_point);
                painter.paint(
                    &mut pixels,
                    w,
                    h,
                    out.pixels_per_point,
                    &primitives,
                    &out.textures_delta,
                );
                std::thread::sleep(Duration::from_millis(100));
            }
            let rgba: Vec<u8> = pixels
                .iter()
                .flat_map(|p| {
                    let [b, g, r, a] = p.to_le_bytes();
                    [r, g, b, a]
                })
                .collect();
            let file = dir.join(format!("{}.png", page.title()));
            image::save_buffer(&file, &rgba, w as u32, h as u32, image::ColorType::Rgba8).unwrap();
        }
    }
}
