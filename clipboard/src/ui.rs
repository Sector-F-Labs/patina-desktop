//! The clipboard menu: a header with Clear all, then the copies, newest first. A click on a
//! row copies it again and closes the menu; × deletes it; dragging a row out drops the copy
//! into the app it's let go on, and the menu closes when the button's let go, not before
//! (ADR-0140); Pin keeps it open. Images show a thumbnail.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::{SystemTime, UNIX_EPOCH};

use egui::{
    Align, ColorImage, Layout, Rect, RichText, ScrollArea, Sense, StrokeKind, TextureHandle, Ui,
    UiBuilder, pos2, vec2,
};
use patina::skin::Skin;
use patina_shell::{Drag, Icon, Shell};
use patina_system::clipboard::{self, Entry, Kind, Status};

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
    /// A drag the host is to start from the press that's held.
    drag: Option<Drag>,
    /// A drag from here is under way: the menu is left alone until it ends.
    dragging: bool,
    /// Pinned: it stays open when focus moves away, after a drop and after a pick, until
    /// Escape or the bar's icon.
    pinned: bool,
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
            drag: None,
            dragging: false,
            pinned: false,
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
        self.done = !self.pinned;
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
            ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click_and_drag());
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
        if response.drag_started() && !cross_response.is_pointer_button_down_on() {
            self.drag = drag_of(ui.ctx(), &self.skin, entry);
        }
        if cross_response.clicked() {
            self.deleted(entry.id);
        } else if response.clicked() {
            self.picked(entry.id);
        }
    }
}

/// A drag of `entry`: the types it holds, their bytes asked of the compositor when the app
/// it's dropped on wants them, and a picture of the row to show under the pointer.
fn drag_of(ctx: &egui::Context, skin: &Skin, entry: &Entry) -> Option<Drag> {
    let types = match clipboard::types(entry.id) {
        // A type can be listed twice (an app offers it under two atoms): offered once.
        Ok(mut types) if !types.is_empty() => {
            let mut seen = std::collections::HashSet::new();
            types.retain(|t| seen.insert(t.clone()));
            types
        }
        Ok(_) => return None,
        Err(e) => {
            eprintln!("patina-clipboard: {e}");
            return None;
        }
    };
    let id = entry.id;
    Some(Drag {
        types,
        data: std::sync::Arc::new(move |mime| clipboard::content(id, mime).ok()),
        icon: drag_icon(skin, entry, ctx.pixels_per_point()),
    })
}

/// The logical width of the picture under the pointer while dragging.
const ICON_WIDTH: f32 = 240.0;

/// `entry`'s row as a small card, drawn here and now at `ppp` pixels to the point, in the
/// menu's skin.
fn drag_icon(skin: &Skin, entry: &Entry, ppp: f32) -> Option<Icon> {
    let height = if entry.thumbnail.is_some() {
        IMAGE_ROW
    } else {
        ROW
    };
    let scale = ppp.ceil().max(1.0);
    let ctx = egui::Context::default();
    skin.apply(&ctx);
    ctx.set_pixels_per_point(scale);
    let image = entry
        .thumbnail
        .as_deref()
        .and_then(|path| image::open(path).ok())
        .map(|img| {
            let img = img.to_rgba8();
            let size = [img.width() as usize, img.height() as usize];
            let colours = ColorImage::from_rgba_unmultiplied(size, img.as_raw());
            ctx.load_texture("drag-icon", colours, Default::default())
        });
    let (w, h) = ((ICON_WIDTH * scale) as usize, (height * scale) as usize);
    let mut painter = patina_shell::raster::Painter::default();
    let mut pixels = vec![0u32; w * h];
    // The first frame loads the fonts and the picture; the last is the one that's right.
    for _ in 0..3 {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(
                pos2(0.0, 0.0),
                vec2(ICON_WIDTH, height),
            )),
            focused: true,
            max_texture_side: Some(8192),
            ..Default::default()
        };
        let out = ctx.run_ui(input, |ui| {
            let rect = ui.max_rect().shrink(1.0);
            let v = ui.visuals().clone();
            ui.painter().rect(
                rect,
                v.widgets.hovered.corner_radius,
                v.panel_fill,
                v.window_stroke,
                StrokeKind::Inside,
            );
            let mut left = rect.left() + 10.0;
            if let Some(texture) = &image {
                let size = texture.size_vec2();
                let k = ((height - 14.0) / size.y).min(1.0).min(96.0 / size.x);
                let shown =
                    Rect::from_min_size(pos2(left, rect.center().y - size.y * k / 2.0), size * k);
                ui.painter().image(
                    texture.id(),
                    shown,
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
                left = shown.right() + 10.0;
            }
            let mut job = egui::text::LayoutJob::simple_singleline(
                entry.preview.clone(),
                egui::TextStyle::Body.resolve(ui.style()),
                v.text_color(),
            );
            job.wrap =
                egui::text::TextWrapping::truncate_at_width((rect.right() - 10.0 - left).max(20.0));
            let galley = ui.painter().layout_job(job);
            ui.painter().galley(
                pos2(left, rect.center().y - galley.size().y / 2.0),
                galley,
                v.text_color(),
            );
        });
        let primitives = ctx.tessellate(out.shapes, out.pixels_per_point);
        pixels.fill(0);
        painter.paint(
            &mut pixels,
            w,
            h,
            out.pixels_per_point,
            &primitives,
            &out.textures_delta,
        );
    }
    Some(Icon {
        size: (w as u32, h as u32),
        scale: scale as i32,
        pixels,
        hotspot: ((20.0 * scale) as i32, (height / 2.0 * scale) as i32),
    })
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
        } else if self.had_focus && !self.pinned && !self.dragging && self.drag.is_none() {
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
            patina_shell::hud::corner_radius(),
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
                patina::dialog::buttons(ui);
                let any = !status.entries.is_empty();
                if ui
                    .add_enabled(any, egui::Button::new("Clear all"))
                    .clicked()
                {
                    self.cleared();
                }
                ui.checkbox(&mut self.pinned, "Pin")
                    .on_hover_text("Keep the menu open until Escape or the bar's icon");
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

    fn take_drag(&mut self) -> Option<Drag> {
        let drag = self.drag.take();
        self.dragging |= drag.is_some();
        drag
    }

    /// Let go, wherever: the menu has done what it was opened for.
    fn drag_ended(&mut self) {
        self.dragging = false;
        self.done = !self.pinned;
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
    fn pinned_it_stays_open_after_a_pick_and_a_drop() {
        let (mut m, _commands) = menu();
        m.pinned = true;
        m.picked(1);
        assert!(!m.finished());
        m.dragging = true;
        m.drag_ended();
        assert!(!m.finished());
        m.pinned = false;
        m.drag_ended();
        assert!(m.finished(), "unpinned, the drop closes it");
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
    fn a_drag_icon_is_a_card_with_the_copy_on_it() {
        let icon = drag_icon(&Skin::default_skin(), &entry(1), 2.0).unwrap();
        assert_eq!(icon.size, (480, 72));
        assert_eq!(icon.scale, 2);
        assert_eq!(icon.pixels.len(), 480 * 72);
        assert!(
            icon.pixels.iter().any(|p| p >> 24 != 0),
            "something is drawn"
        );
        assert_eq!(icon.pixels[0] >> 24, 0, "the corner is clear");
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
