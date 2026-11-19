//! `viewer [files…]`: the corpus in an egui window, rendered by Blitz into a
//! texture, with pointer events passed back to Blitz for hover, selection and
//! link clicks. Ctrl+C copies the selected text.

use std::collections::HashMap;
use std::path::PathBuf;

use blitz_spike::Message;
use blitz_traits::events::{BlitzPointerEvent, MouseEventButtons, UiEvent};

struct Viewer {
    files: Vec<PathBuf>,
    current: usize,
    message: Option<Message>,
    texture: Option<egui::TextureHandle>,
    /// The width and scale the message was laid out and painted at.
    laid_out: (u32, f32),
    dirty: bool,
    clicked: Vec<String>,
    timing: String,
}

impl Viewer {
    fn load(&mut self, width: u32, scale: f32, dark: bool) {
        let html = std::fs::read_to_string(&self.files[self.current]).unwrap_or_default();
        let mut parts = HashMap::new();
        if let Ok(png) = std::fs::read("../../assets/mail-icon-128.png") {
            parts.insert("chart@acme".to_string(), png.into());
        }
        let t = std::time::Instant::now();
        self.message = Some(Message::new(&html, parts, width, scale, dark));
        self.timing = format!("layout {:.0?}", t.elapsed());
        self.laid_out = (width, scale);
        self.dirty = true;
    }

    fn paint(&mut self, ctx: &egui::Context) {
        let Some(m) = &mut self.message else { return };
        let (width, scale) = self.laid_out;
        let height = (m.height() * scale).ceil().max(1.0) as u32;
        let t = std::time::Instant::now();
        let rgba = m.paint(width, 0.0, height, scale);
        let w = (width as f32 * scale) as usize;
        let image = egui::ColorImage::from_rgba_unmultiplied([w, height as usize], &rgba);
        self.texture = Some(ctx.load_texture("message", image, egui::TextureOptions::NEAREST));
        self.timing = format!("{}, paint {:.0?}", self.timing.split(',').next().unwrap_or(""), t.elapsed());
        self.dirty = false;
    }

    fn pointer(&mut self, event: fn(BlitzPointerEvent) -> UiEvent, pos: egui::Pos2, buttons: MouseEventButtons) {
        let Some(m) = &mut self.message else { return };
        m.pointer(event, pos.x, pos.y, buttons == MouseEventButtons::Primary);
        self.dirty = true;
    }
}

impl eframe::App for Viewer {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let mut open = None;
        egui::Panel::left("files").show(ui, |ui| {
            for (i, f) in self.files.iter().enumerate() {
                let name = f.file_name().unwrap().to_string_lossy();
                if ui.selectable_label(i == self.current, name).clicked() {
                    open = Some(i);
                }
            }
        });
        let hovered = self.message.as_ref().and_then(Message::hovered_link);
        egui::Panel::bottom("status").show(ui, |ui| {
            if let Some(m) = &self.message {
                let refused = m.guard.refused.lock().unwrap();
                ui.horizontal_wrapped(|ui| {
                    ui.label(format!("{} · blocked {}", self.timing, refused.len())).on_hover_text(refused.join("\n"));
                    if let Some(link) = &hovered {
                        ui.separator();
                        ui.label(format!("→ {link}"));
                    }
                });
                for c in m.links.clicked.lock().unwrap().iter() {
                    ui.label(format!("caught {c:?} (not followed)"));
                }
            }
        });
        egui::CentralPanel::default().show(ui, |ui| {
            let width = ui.available_width().floor() as u32;
            let scale = ctx.pixels_per_point();
            if let Some(i) = open {
                self.current = i;
                self.message = None;
            }
            if self.message.is_none() || self.laid_out != (width, scale) {
                self.load(width, scale, false);
            }
            if self.dirty {
                self.paint(&ctx);
            }
            let Some(tex) = self.texture.clone() else { return };
            let size = tex.size_vec2() / scale;
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                let resp = ui.add(egui::Image::new((tex.id(), size)).sense(egui::Sense::click_and_drag()));
                let to_doc = |p: egui::Pos2| (p - resp.rect.min).to_pos2();
                let (pressed, released, down) =
                    ctx.input(|i| (i.pointer.primary_pressed(), i.pointer.primary_released(), i.pointer.primary_down()));
                if let Some(p) = resp.hover_pos().or(ctx.pointer_interact_pos().filter(|_| down)) {
                    let buttons = if down { MouseEventButtons::Primary } else { MouseEventButtons::None };
                    if ctx.input(|i| i.pointer.delta() != egui::Vec2::ZERO) {
                        self.pointer(UiEvent::PointerMove, to_doc(p), buttons);
                    }
                    if pressed && resp.hovered() {
                        self.pointer(UiEvent::PointerDown, to_doc(p), MouseEventButtons::Primary);
                    }
                    if released {
                        self.pointer(UiEvent::PointerUp, to_doc(p), MouseEventButtons::None);
                    }
                }
                if let Some(cursor) = self.message.as_ref().and_then(|m| m.doc.get_cursor()) {
                    let icon = match format!("{cursor:?}").as_str() {
                        "Pointer" => egui::CursorIcon::PointingHand,
                        "Text" => egui::CursorIcon::Text,
                        _ => egui::CursorIcon::Default,
                    };
                    if resp.hovered() {
                        ctx.set_cursor_icon(icon);
                    }
                }
            });
        });
        // SPIKE_SCREENSHOT=path.png: save the window after a moment, and quit.
        if let Ok(path) = std::env::var("SPIKE_SCREENSHOT") {
            ctx.request_repaint();
            if let Some(image) = ctx.input(|i| {
                i.raw.events.iter().find_map(|e| match e {
                    egui::Event::Screenshot { image, .. } => Some(image.clone()),
                    _ => None,
                })
            }) {
                let [w, h] = image.size;
                image::RgbaImage::from_raw(w as u32, h as u32, image.as_raw().to_vec()).unwrap().save(&path).unwrap();
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else if ctx.cumulative_frame_nr() == 30 {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            }
        }
        if ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Copy)))
            && let Some(text) = self.message.as_ref().and_then(|m| m.doc.get_selected_text())
        {
            ctx.copy_text(text);
        }
        if let Some(m) = &self.message {
            let clicked: Vec<String> = m.links.clicked.lock().unwrap().iter().map(|c| c.1.clone()).collect();
            if clicked.len() != self.clicked.len() {
                self.clicked = clicked;
            }
        }
    }
}

fn main() -> eframe::Result {
    let mut files: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    if files.is_empty() {
        files = std::fs::read_dir("corpus").unwrap().filter_map(|e| Some(e.ok()?.path())).collect();
        files.sort();
    }
    let viewer = Viewer {
        files,
        current: 0,
        message: None,
        texture: None,
        laid_out: (0, 0.0),
        dirty: false,
        clicked: Vec::new(),
        timing: String::new(),
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("Blitz spike").with_inner_size([1000.0, 760.0]),
        ..Default::default()
    };
    eframe::run_native("blitz-spike", options, Box::new(|_| Ok(Box::new(viewer))))
}
