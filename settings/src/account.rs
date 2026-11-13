//! The Account page: the user's picture (ADR-0104). The Sector F flask in
//! each skin's colours, any image in Pictures/Avatars, or none; the choice
//! is written to `~/.face`, which the lock screen shows.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc;

use egui::{Color32, Rect, RichText, Sense, Stroke, TextureHandle, Ui, pos2, vec2};
use patina::skin::Skin;
use patina_system::avatar;

use crate::app::Message;
use crate::wallpapers;

/// A picture's circle, in points, and its name under it.
const DISC: f32 = 88.0;
const LABEL: f32 = 20.0;

/// A skin's flask picture: the colours it's drawn in.
struct Flask {
    name: String,
    background: Color32,
    mark: Color32,
    /// The picture it makes, to tell whether it's the one in use.
    image: egui::ColorImage,
}

impl Flask {
    fn picture(&self) -> image::RgbaImage {
        avatar::sector_f(self.background.to_array(), self.mark.to_array())
    }
}

pub struct AccountPage {
    flasks: Vec<Flask>,
    /// The user's picture now, as a texture; reloaded after a change.
    current: Option<TextureHandle>,
    current_image: Option<egui::ColorImage>,
    /// Which of the user's own pictures is the one in use, once found.
    own_in_use: Option<PathBuf>,
    matching: Option<mpsc::Receiver<PathBuf>>,
    loaded: bool,
    flask_textures: HashMap<String, TextureHandle>,
    own: Vec<wallpapers::Wallpaper>,
    thumbnails: HashMap<PathBuf, Option<TextureHandle>>,
    decoded: Option<mpsc::Receiver<(PathBuf, Option<egui::ColorImage>)>>,
    /// The file chooser, while it's open: the file picked, or `None` if it
    /// was closed without one.
    choosing: Option<mpsc::Receiver<Option<PathBuf>>>,
}

/// The folder for the user's own pictures, `~/Pictures/Avatars`.
fn user_dir() -> Option<PathBuf> {
    dirs::picture_dir().map(|d| d.join("Avatars"))
}

impl AccountPage {
    pub fn new(skins: &[Skin]) -> Self {
        // A skin's colours are what it sets on an egui context.
        let flasks = skins
            .iter()
            .map(|skin| {
                let ctx = egui::Context::default();
                skin.apply(&ctx);
                let v = ctx.global_style().visuals.clone();
                let mut flask = Flask {
                    name: skin.name.clone(),
                    background: v.selection.bg_fill,
                    mark: v.selection.stroke.color,
                    image: egui::ColorImage::default(),
                };
                flask.image = egui::ColorImage::from_rgba_unmultiplied(
                    [avatar::SIZE as usize; 2],
                    flask.picture().as_raw(),
                );
                flask
            })
            .collect();
        Self {
            flasks,
            current: None,
            current_image: None,
            own_in_use: None,
            matching: None,
            loaded: false,
            flask_textures: HashMap::new(),
            own: Vec::new(),
            thumbnails: HashMap::new(),
            decoded: None,
            choosing: None,
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, message: &mut Option<Message>) {
        if self.decoded.is_none() {
            self.own = user_dir()
                .map(|d| wallpapers::images(&d))
                .unwrap_or_default();
            let paths = self.own.iter().map(|w| w.path.clone()).collect();
            self.decoded = Some(wallpapers::thumbnails(paths, ui.ctx().clone()));
        }
        if !self.loaded {
            self.loaded = true;
            self.current_image = avatar::load();
            self.current = self
                .current_image
                .clone()
                .map(|i| ui.ctx().load_texture("avatar", i, Default::default()));
            self.own_in_use = None;
            self.matching = self.current_image.clone().map(|current| {
                let paths: Vec<PathBuf> = self.own.iter().map(|w| w.path.clone()).collect();
                let ctx = ui.ctx().clone();
                let (send, receive) = mpsc::channel();
                let _ = std::thread::Builder::new()
                    .name("avatar-match".into())
                    .spawn(move || {
                        for path in paths {
                            let Ok(image) = image::open(&path) else {
                                continue;
                            };
                            let square = avatar::square(image);
                            let square = egui::ColorImage::from_rgba_unmultiplied(
                                [avatar::SIZE as usize; 2],
                                square.as_raw(),
                            );
                            if square.pixels == current.pixels {
                                let _ = send.send(path);
                                ctx.request_repaint();
                                return;
                            }
                        }
                    });
                receive
            });
        }
        if let Some(matching) = &self.matching
            && let Ok(path) = matching.try_recv()
        {
            self.own_in_use = Some(path);
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

        let flask_in_use = self.current_image.as_ref().and_then(|current| {
            self.flasks
                .iter()
                .position(|f| f.image.pixels == current.pixels)
        });
        let description = match (&self.current_image, flask_in_use, &self.own_in_use) {
            (None, _, _) => "No picture: the lock screen shows your initial".to_string(),
            (_, Some(i), _) => format!("Sector F, {}", self.flasks[i].name),
            (_, _, Some(path)) => path
                .file_stem()
                .map(|n| n.to_string_lossy().replace('_', " "))
                .unwrap_or_default(),
            _ => "A picture of your own".to_string(),
        };
        let mut chosen: Option<Result<image::RgbaImage, String>> = None;
        if let Some(choosing) = &self.choosing
            && let Ok(picked) = choosing.try_recv()
        {
            self.choosing = None;
            if let Some(path) = picked {
                chosen = Some(
                    image::open(&path)
                        .map(avatar::square)
                        .map_err(|e| format!("{}: {e}", path.display())),
                );
            }
        }
        let mut removed = false;
        ui.heading("Account");
        ui.label("Your picture, on the lock screen.");
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(DISC, DISC), Sense::hover());
            match &self.current {
                Some(texture) => disc(ui, rect, texture),
                None => {
                    ui.painter().circle_filled(
                        rect.center(),
                        DISC / 2.0,
                        ui.visuals().faint_bg_color,
                    );
                    ui.painter().circle_stroke(
                        rect.center(),
                        DISC / 2.0,
                        ui.visuals().widgets.noninteractive.bg_stroke,
                    );
                }
            }
            ui.vertical(|ui| {
                ui.add_space(DISC / 2.0 - 24.0);
                if self.current.is_some() {
                    ui.label(RichText::new("In use").small().weak());
                }
                ui.label(RichText::new(description).strong());
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            self.choosing.is_none(),
                            egui::Button::new("Choose a picture…"),
                        )
                        .clicked()
                    {
                        self.choosing = Some(choose(ui.ctx().clone()));
                    }
                    if ui
                        .add_enabled(self.current.is_some(), egui::Button::new("Remove"))
                        .clicked()
                    {
                        removed = true;
                    }
                });
            });
        });
        ui.add_space(12.0);

        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Frame::NONE.inner_margin(4.0).show(ui, |ui| {
                ui.label(RichText::new("Sector F").strong());
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(12.0, 12.0);
                    for (i, flask) in self.flasks.iter().enumerate() {
                        let texture = self
                            .flask_textures
                            .entry(flask.name.clone())
                            .or_insert_with(|| {
                                ui.ctx().load_texture(
                                    format!("flask-{}", flask.name),
                                    flask.image.clone(),
                                    Default::default(),
                                )
                            })
                            .clone();
                        let on = flask_in_use == Some(i);
                        if tile(ui, &flask.name, on, |ui, rect| disc(ui, rect, &texture)).clicked()
                        {
                            chosen = Some(Ok(flask.picture()));
                        }
                    }
                });
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Your own").strong());
                    if let Some(dir) = user_dir()
                        && ui
                            .button("Open folder")
                            .on_hover_text(dir.display().to_string())
                            .clicked()
                    {
                        let _ = std::fs::create_dir_all(&dir);
                        if let Err(e) = patina::open::path(&dir) {
                            *message = Some(Message::error(format!("Couldn't open it: {e}")));
                        }
                    }
                });
                ui.label(
                    RichText::new(
                        "Pictures in Pictures/Avatars. Any picture is cropped to a square.",
                    )
                    .weak(),
                );
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(12.0, 12.0);
                    for w in &self.own {
                        let thumbnail = self.thumbnails.get(&w.path);
                        let on = self.own_in_use.as_ref() == Some(&w.path);
                        let response = tile(ui, &w.name, on, |ui, rect| match thumbnail {
                            Some(Some(texture)) => disc(ui, rect, texture),
                            _ => {
                                ui.painter().circle_filled(
                                    rect.center(),
                                    DISC / 2.0,
                                    ui.visuals().faint_bg_color,
                                );
                                if thumbnail.is_none() {
                                    ui.put(rect, egui::Spinner::new());
                                }
                            }
                        });
                        if response.clicked() {
                            chosen = Some(
                                image::open(&w.path)
                                    .map(avatar::square)
                                    .map_err(|e| format!("{}: {e}", w.path.display())),
                            );
                        }
                    }
                });
            });
        });

        let result = if removed {
            Some(avatar::remove())
        } else {
            chosen.map(|c| c.and_then(|picture| avatar::save(&picture)))
        };
        match result {
            Some(Ok(())) => {
                self.loaded = false;
                // The picture is reloaded and the status bar shows the
                // message on the next frame, which nothing else would ask for.
                ui.ctx().request_repaint();
                *message = Some(Message::note(if removed {
                    "Picture removed"
                } else {
                    "Picture set"
                }));
            }
            Some(Err(e)) => {
                *message = Some(Message::error(format!("Couldn't save it: {e}")));
                ui.ctx().request_repaint();
            }
            None => {}
        }
    }
}

/// Opens the desktop's file chooser (the portal: Files, in the Patina
/// session) on a thread of its own, and sends the image picked, or `None`.
fn choose(ctx: egui::Context) -> mpsc::Receiver<Option<PathBuf>> {
    let (send, receive) = mpsc::channel();
    let _ = std::thread::Builder::new()
        .name("file-chooser".into())
        .spawn(move || {
            let mut dialog = rfd::FileDialog::new()
                .set_title("Choose a picture")
                .add_filter("Images", &["png", "jpg", "jpeg", "webp"]);
            if let Some(dir) = dirs::picture_dir() {
                dialog = dialog.set_directory(dir);
            }
            let _ = send.send(dialog.pick_file());
            ctx.request_repaint();
        });
    receive
}

/// `texture` in a circle filling `rect`.
fn disc(ui: &Ui, rect: Rect, texture: &TextureHandle) {
    egui::Image::from_texture(texture)
        .corner_radius(rect.width().min(rect.height()) / 2.0)
        .paint_at(ui, rect);
}

/// A picture's circle drawn by `draw`, with its name under it.
/// Ringed, with a tick on it, when it's the one in use.
fn tile(ui: &mut Ui, name: &str, in_use: bool, draw: impl FnOnce(&mut Ui, Rect)) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(DISC, DISC + LABEL), Sense::click());
    let picture = Rect::from_min_size(rect.min, vec2(DISC, DISC));
    draw(ui, picture);
    let v = ui.visuals();
    if in_use {
        let painter = ui.painter();
        painter.circle_stroke(
            picture.center(),
            DISC / 2.0 + 2.0,
            Stroke::new(3.0, v.selection.bg_fill),
        );
        let badge = pos2(picture.right() - 10.0, picture.bottom() - 10.0);
        painter.circle_filled(badge, 11.0, v.selection.bg_fill);
        let tick = Stroke::new(2.0, v.selection.stroke.color);
        painter.line_segment([badge + vec2(-5.0, 0.0), badge + vec2(-1.5, 3.5)], tick);
        painter.line_segment([badge + vec2(-1.5, 3.5), badge + vec2(5.0, -4.0)], tick);
    } else if response.hovered() {
        ui.painter().circle_stroke(
            picture.center(),
            DISC / 2.0,
            Stroke::new(2.0, v.selection.bg_fill),
        );
    }
    ui.painter().text(
        pos2(rect.center().x, picture.bottom() + LABEL / 2.0),
        egui::Align2::CENTER_CENTER,
        name,
        egui::TextStyle::Small.resolve(ui.style()),
        v.text_color(),
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs a frame of the page and says whether it asked for another at once.
    fn frame(ctx: &egui::Context, page: &mut AccountPage, message: &mut Option<Message>) -> bool {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(900.0, 700.0))),
            ..Default::default()
        };
        let out = ctx.run_ui(input, |ui| page.ui(ui, message));
        out.viewport_output
            .values()
            .any(|v| v.repaint_delay.is_zero())
    }

    #[test]
    fn a_picture_chosen_in_the_file_chooser_becomes_the_picture_and_shows() {
        let home = std::env::temp_dir().join(format!("patina-account-{}", std::process::id()));
        std::fs::create_dir_all(&home).unwrap();
        // SAFETY: no other test here reads HOME.
        unsafe { std::env::set_var("HOME", &home) };
        let chosen = home.join("wide.jpg");
        image::RgbImage::from_pixel(600, 300, image::Rgb([200, 30, 30]))
            .save(&chosen)
            .unwrap();

        let ctx = egui::Context::default();
        let mut page = AccountPage::new(&[]);
        let mut message = None;
        frame(&ctx, &mut page, &mut message);
        assert!(page.current.is_none());

        // The chooser answers.
        let (send, receive) = mpsc::channel();
        send.send(Some(chosen)).unwrap();
        page.choosing = Some(receive);
        let again = frame(&ctx, &mut page, &mut message);

        assert!(avatar::path().unwrap().exists(), "not saved");
        // Asks for a frame to show it, rather than waiting for the mouse.
        assert!(again, "no repaint asked for after the picture was saved");
        frame(&ctx, &mut page, &mut message);
        assert!(page.current.is_some(), "the new picture isn't shown");
        let _ = std::fs::remove_dir_all(&home);
    }
}
