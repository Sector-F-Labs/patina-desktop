//! The clipboard menu: a header with Clear all, then the copies, newest first. A click on a
//! row copies it again and closes the menu; × deletes it. Images show a thumbnail.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::{SystemTime, UNIX_EPOCH};

use egui::{
    Align, ColorImage, Layout, Rect, RichText, ScrollArea, Sense, StrokeKind, TextureHandle, Ui,
    UiBuilder, pos2, vec2,
};
use patina::skin::Skin;
use patina_shell::Shell;
use patina_system::clipboard::{Entry, Kind, Status};

const MARGIN: f32 = 12.0;
const ROW: f32 = 36.0;
const IMAGE_ROW: f32 = 72.0;
/// The side of the × button.
const CROSS: f32 = 24.0;

/// What the menu asks of the compositor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Pick(u64),
    Delete(u64),
    Clear,
    Paused(bool),
}

pub struct ClipboardMenu {
    /// `None` until the compositor has answered, and while it doesn't serve the history.
    pub status: Option<Status>,
    /// Thumbnails read so far; `None` for a file that wouldn't load.
    thumbnails: HashMap<PathBuf, Option<TextureHandle>>,
    had_focus: bool,
    done: bool,
    skin: Skin,
    commands: Sender<Command>,
}

impl ClipboardMenu {
    pub fn new(skin: Skin, commands: Sender<Command>) -> Self {
        Self {
            status: None,
            thumbnails: HashMap::new(),
            had_focus: false,
            done: false,
            skin,
            commands,
        }
    }

    pub fn close(&mut self) {
        self.done = true;
    }

    fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }

    /// A click on entry `id`: it's copied again, and the menu is done.
    fn picked(&mut self, id: u64) {
        self.send(Command::Pick(id));
        self.done = true;
    }

    /// The ×: the entry goes at once from the list, and from the compositor's.
    fn deleted(&mut self, id: u64) {
        self.send(Command::Delete(id));
        if let Some(status) = self.status.as_mut() {
            status.entries.retain(|e| e.id != id);
        }
    }

    fn cleared(&mut self) {
        self.send(Command::Clear);
        if let Some(status) = self.status.as_mut() {
            status.entries.clear();
        }
    }

    /// `path`'s thumbnail, read the first time it's asked for: a few kilobytes at most.
    fn thumbnail(&mut self, ctx: &egui::Context, path: &Path) -> Option<TextureHandle> {
        self.thumbnails
            .entry(path.to_path_buf())
            .or_insert_with(|| {
                let image = image::open(path).ok()?.to_rgba8();
                let size = [image.width() as usize, image.height() as usize];
                let colours = ColorImage::from_rgba_unmultiplied(size, image.as_raw());
                Some(ctx.load_texture(path.to_string_lossy(), colours, Default::default()))
            })
            .clone()
    }

    fn row(&mut self, ui: &mut Ui, entry: &Entry, now: u32) {
        let height = if entry.thumbnail.is_some() {
            IMAGE_ROW
        } else {
            ROW
        };
        let (rect, response) =
            ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
        let cross = Rect::from_center_size(
            rect.right_center() - vec2(CROSS / 2.0 + 4.0, 0.0),
            vec2(CROSS, CROSS),
        );
        let cross_response = ui.interact(cross, response.id.with("delete"), Sense::click());
        let v = ui.visuals().clone();
        if response.hovered() || cross_response.hovered() {
            ui.painter().rect_filled(
                rect,
                v.widgets.hovered.corner_radius,
                v.widgets.hovered.weak_bg_fill,
            );
        }
        let mut left = rect.left() + 10.0;
        if let Some(path) = &entry.thumbnail
            && let Some(texture) = self.thumbnail(ui.ctx(), path)
        {
            let size = texture.size_vec2();
            let scale = ((height - 12.0) / size.y).min(1.0).min(96.0 / size.x);
            let shown = Rect::from_min_size(
                pos2(left, rect.center().y - size.y * scale / 2.0),
                size * scale,
            );
            ui.painter().image(
                texture.id(),
                shown,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
            left = shown.right() + 10.0;
        }
        let ago = ago(now, entry.at);
        let small = egui::TextStyle::Small.resolve(ui.style());
        let ago_galley = ui.painter().layout_no_wrap(ago, small, v.weak_text_color());
        let right = cross.left() - 8.0;
        let ago_at = pos2(
            right - ago_galley.size().x,
            rect.center().y - ago_galley.size().y / 2.0,
        );
        let text_width = (ago_at.x - 8.0 - left).max(20.0);
        ui.painter().galley(ago_at, ago_galley, v.weak_text_color());
        let font = egui::TextStyle::Body.resolve(ui.style());
        let galley = ui
            .painter()
            .layout(entry.preview.clone(), font, v.text_color(), text_width);
        // One line, cut where it stops fitting.
        let galley = if galley.rows.len() > 1 {
            let mut job = egui::text::LayoutJob::simple_singleline(
                entry.preview.clone(),
                egui::TextStyle::Body.resolve(ui.style()),
                v.text_color(),
            );
            job.wrap = egui::text::TextWrapping::truncate_at_width(text_width);
            ui.painter().layout_job(job)
        } else {
            galley
        };
        let kind_colour = match entry.kind {
            Kind::Image => v.weak_text_color(),
            _ => v.text_color(),
        };
        ui.painter().galley(
            pos2(left, rect.center().y - galley.size().y / 2.0),
            galley,
            kind_colour,
        );
        // The ×, lit under the pointer.
        let cross_colour = if cross_response.hovered() {
            v.strong_text_color()
        } else {
            v.weak_text_color()
        };
        let c = cross.center();
        let d = 4.5;
        let stroke = egui::Stroke::new(1.5, cross_colour);
        ui.painter()
            .line_segment([c + vec2(-d, -d), c + vec2(d, d)], stroke);
        ui.painter()
            .line_segment([c + vec2(-d, d), c + vec2(d, -d)], stroke);
        cross_response.clone().on_hover_text("Delete");
        if cross_response.clicked() {
            self.deleted(entry.id);
        } else if response.clicked() {
            self.picked(entry.id);
        }
    }
}

/// How long before `now` an entry from `at` was: "now", "5m", "3h", "2d".
fn ago(now: u32, at: u32) -> String {
    let s = now.saturating_sub(at);
    match s {
        0..=9 => "now".into(),
        10..=59 => format!("{s}s"),
        60..=3599 => format!("{}m", s / 60),
        3600..=86399 => format!("{}h", s / 3600),
        _ => format!("{}d", s / 86400),
    }
}

fn now() -> u32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u32::try_from(d.as_secs()).unwrap_or(u32::MAX))
}

/// Buttons as big as a window's title bar and tool bar have them (patina-ui's
/// bar buttons): as wide as their text plus 10, 24 high, not egui's tiny ones.
fn bar_buttons(ui: &mut Ui) {
    let spacing = ui.spacing_mut();
    spacing.button_padding = vec2(10.0, 5.0);
    spacing.interact_size.y = 24.0;
    spacing.item_spacing.x = 8.0;
}

impl Shell for ClipboardMenu {
    fn setup(&mut self, ctx: &egui::Context) {
        self.skin.apply(ctx);
        if self.skin.follows_desktop() {
            patina::appearance::watch(ctx);
        }
    }

    fn ui(&mut self, ui: &mut Ui, _: &str) {
        if ui.input(|i| i.focused) {
            self.had_focus = true;
        } else if self.had_focus {
            self.done = true;
            return;
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.done = true;
            return;
        }

        let rect = ui.max_rect();
        let v = ui.visuals().clone();
        ui.painter().rect(
            rect,
            v.menu_corner_radius,
            v.panel_fill,
            v.window_stroke,
            StrokeKind::Inside,
        );
        let mut ui = ui.new_child(
            UiBuilder::new()
                .max_rect(rect.shrink(MARGIN))
                .layout(Layout::top_down(Align::Min)),
        );

        let Some(status) = self.status.clone() else {
            ui.weak("The compositor keeps no clipboard history here.");
            return;
        };
        ui.horizontal(|ui| {
            ui.heading("Clipboard");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                bar_buttons(ui);
                let any = !status.entries.is_empty();
                if ui
                    .add_enabled(any, egui::Button::new("Clear all"))
                    .clicked()
                {
                    self.cleared();
                }
                let mut paused = status.paused;
                if ui.checkbox(&mut paused, "Pause").changed() {
                    self.send(Command::Paused(paused));
                    if let Some(s) = self.status.as_mut() {
                        s.paused = paused;
                    }
                }
            });
        });
        if !status.enabled {
            ui.label(RichText::new("History is turned off.").weak());
        } else if status.paused {
            ui.label(RichText::new("Paused: new copies aren't kept.").weak());
        }
        ui.separator();
        if status.entries.is_empty() {
            ui.weak("Nothing copied yet.");
            return;
        }
        let now = now();
        ScrollArea::vertical()
            .auto_shrink(false)
            .show(&mut ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for entry in &status.entries {
                    self.row(ui, entry, now);
                }
            });
    }

    fn finished(&self) -> bool {
        self.done
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn entry(id: u64) -> Entry {
        Entry {
            id,
            kind: Kind::Text,
            preview: format!("copy {id}"),
            thumbnail: None,
            at: 100,
        }
    }

    fn menu() -> (ClipboardMenu, mpsc::Receiver<Command>) {
        let (send, receive) = mpsc::channel();
        let mut m = ClipboardMenu::new(Skin::default_skin(), send);
        m.status = Some(Status {
            entries: vec![entry(2), entry(1)],
            enabled: true,
            paused: false,
        });
        (m, receive)
    }

    #[test]
    fn picking_asks_for_it_and_closes() {
        let (mut m, commands) = menu();
        m.picked(1);
        assert_eq!(commands.try_recv(), Ok(Command::Pick(1)));
        assert!(m.finished());
    }

    #[test]
    fn deleting_and_clearing_empty_the_list_at_once() {
        let (mut m, commands) = menu();
        m.deleted(2);
        assert_eq!(commands.try_recv(), Ok(Command::Delete(2)));
        assert_eq!(m.status.as_ref().unwrap().entries.len(), 1);
        assert!(!m.finished(), "deleting leaves it open");
        m.cleared();
        assert_eq!(commands.try_recv(), Ok(Command::Clear));
        assert!(m.status.as_ref().unwrap().entries.is_empty());
    }

    #[test]
    fn ages_are_short() {
        assert_eq!(ago(100, 100), "now");
        assert_eq!(ago(130, 100), "30s");
        assert_eq!(ago(100 + 300, 100), "5m");
        assert_eq!(ago(100 + 7200, 100), "2h");
        assert_eq!(ago(100 + 3 * 86400, 100), "3d");
        assert_eq!(ago(50, 100), "now", "a clock set back");
    }

    /// Draws the menu in each built-in skin, as PNGs in the folder
    /// PATINA_CLIPBOARD_PREVIEW names, from made-up copies:
    /// `PATINA_CLIPBOARD_PREVIEW=/tmp/p cargo test -p patina-clipboard preview -- --ignored`.
    #[test]
    #[ignore]
    fn preview_the_menu() {
        let dir = std::path::PathBuf::from(std::env::var("PATINA_CLIPBOARD_PREVIEW").unwrap());
        std::fs::create_dir_all(&dir).unwrap();
        let (skins, _) = patina::skin::load_all(patina::skin::BUILT_IN, &[]);
        let (w, h) = (crate::SIZE.0 as usize, crate::SIZE.1 as usize);
        let copies = [
            ("cargo test -p patina-clipboard", 20),
            ("https://github.com/Sector-F-Labs/patina-desktop", 140),
            (
                "Authentication is required to run the GParted Partition Editor as root",
                900,
            ),
            ("fn bar_buttons(ui: &mut Ui)", 7200),
        ];
        for skin in skins {
            let name = skin.name.clone();
            let (send, _commands) = mpsc::channel();
            let mut m = ClipboardMenu::new(skin, send);
            m.had_focus = true;
            m.status = Some(Status {
                entries: copies
                    .iter()
                    .enumerate()
                    .map(|(i, (text, ago))| Entry {
                        id: i as u64,
                        kind: Kind::Text,
                        preview: (*text).into(),
                        thumbnail: None,
                        at: now().saturating_sub(*ago),
                    })
                    .collect(),
                enabled: true,
                paused: false,
            });
            let ctx = egui::Context::default();
            m.setup(&ctx);
            let mut painter = patina_shell::raster::Painter::default();
            let mut pixels = vec![0u32; w * h];
            for _ in 0..3 {
                let input = egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        pos2(0.0, 0.0),
                        vec2(w as f32, h as f32),
                    )),
                    focused: true,
                    max_texture_side: Some(8192),
                    ..Default::default()
                };
                let out = ctx.run_ui(input, |ui| m.ui(ui, ""));
                let primitives = ctx.tessellate(out.shapes, out.pixels_per_point);
                painter.paint(
                    &mut pixels,
                    w,
                    h,
                    out.pixels_per_point,
                    &primitives,
                    &out.textures_delta,
                );
            }
            let rgba: Vec<u8> = pixels
                .iter()
                .flat_map(|p| {
                    let [b, g, r, a] = p.to_le_bytes();
                    [r, g, b, a]
                })
                .collect();
            let file = dir.join(format!("clipboard-{name}.png"));
            image::save_buffer(&file, &rgba, w as u32, h as u32, image::ColorType::Rgba8).unwrap();
        }
    }
}
