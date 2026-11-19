//! The column (ADR-0141): one layer-shell surface down the right edge, as wide
//! as a card plus its margin, with the cards from the top. While any are
//! showing it keeps that width as an exclusive zone, so the compositor's work
//! area, and with it tiled windows, make room; with none it is one pixel wide
//! and keeps nothing.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use egui::{
    Align, CornerRadius, Frame, Layout, Margin, RichText, ScrollArea, Sense, Stroke, Ui, UiBuilder,
    vec2,
};
use patina::skin::Skin;
use patina_shell::calloop::channel;
use patina_shell::{
    Anchor, Drag, Host, Icon, KeyboardInteractivity, Layer, Options, Shell, Toplevel,
    ToplevelRequest, piece,
};

use crate::daemon::{Daemon, Shared};
use crate::store::{self, Note, Urgency};
use crate::thumbs::{Stamps, Thumb, Thumbs};
use crate::{config, fullscreen};

/// A card's width, and the space around the cards, in logical pixels.
const CARD: f32 = 340.0;
const PAD: f32 = 12.0;
const GAP: f32 = 8.0;
/// The surface's width while there are cards.
const WIDTH: u32 = (CARD + 2.0 * PAD) as u32;
/// How often the config is looked at, and so the longest the column sleeps.
const HEARTBEAT: Duration = Duration::from_secs(1);

/// What was done to a card.
#[derive(Debug, PartialEq)]
enum Outcome {
    Nothing,
    Dismiss,
    /// One of its buttons.
    Action(String),
    /// Its Edit button: the picture opens in Draw. The card stays.
    Edit,
    /// The card itself: its default action, if it has one, and its app's window.
    Open,
    /// Pulled away from its file card: a drag into an app starts.
    Drag,
}

struct Column {
    skin: Skin,
    shared: Arc<Shared>,
    daemon: Daemon,
    /// The open windows (id, app id), to bring an app's forward from its card.
    windows: Vec<(u64, String)>,
    requests: Vec<ToplevelRequest>,
    /// The pictures on cards, and a drag the host is to start from the press that's held.
    thumbs: Thumbs,
    /// When the files on cards were last looked at, and what they were: an edited one is read
    /// again for its card.
    stamps: Stamps,
    files_checked: Option<Instant>,
    drag: Option<Drag>,
    /// The card a drag in progress came from, which goes if an app takes it.
    dragged: Option<u32>,
    enabled: bool,
    /// Held back while a fullscreen window has the keyboard.
    hide_in_fullscreen: bool,
    fullscreen: Arc<AtomicBool>,
    config_seen: Option<std::time::SystemTime>,
    config_checked: Option<Instant>,
    /// The surface's height and the cards' (with the margin), from the last
    /// frame: what is below the cards takes no pointer input.
    surface_h: f32,
    content_h: f32,
}

impl Column {
    /// Whether the column has anything to show, and so takes space: not while
    /// switched off, nor while a fullscreen window has the keyboard if that is
    /// to hold them back (they wait, and show when it's left).
    fn showing(&self) -> bool {
        self.enabled
            && !(self.hide_in_fullscreen && self.fullscreen.load(Ordering::Relaxed))
            && !self.shared.store().is_empty()
    }

    /// The config's changes, and the notifications whose time has run out.
    fn tick(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        if self.config_checked.is_none_or(|t| t + HEARTBEAT <= now) {
            self.config_checked = Some(now);
            if let Some(config) = config::reload(&mut self.config_seen) {
                self.enabled = config.enabled;
                self.hide_in_fullscreen = config.hide_in_fullscreen;
                self.shared.enabled.store(config.enabled, Ordering::Relaxed);
                self.shared
                    .screenshots
                    .store(config.screenshots, Ordering::Relaxed);
                if !config.enabled {
                    let ids: Vec<u32> = self.shared.store().notes().iter().map(|n| n.id).collect();
                    for id in ids {
                        self.shared.store().close(id);
                        self.daemon.closed(id, store::DISMISSED);
                    }
                }
            }
        }
        if self.files_checked.is_none_or(|t| t + HEARTBEAT <= now) {
            self.files_checked = Some(now);
            let files: Vec<std::path::PathBuf> = self
                .shared
                .store()
                .notes()
                .iter()
                .filter_map(|n| n.image.clone().or_else(|| n.file.clone()))
                .collect();
            let files: Vec<&std::path::Path> = files.iter().map(|f| f.as_path()).collect();
            for changed in self.stamps.changed(&files) {
                self.thumbs.reload(&changed);
            }
        }
        let gone = self.shared.store().expire(now);
        for id in gone {
            self.daemon.closed(id, store::EXPIRED);
        }
        let mut wait = HEARTBEAT;
        if let Some(t) = self.shared.store().next_expiry() {
            wait = wait.min(t.saturating_duration_since(now));
        }
        ctx.request_repaint_after(wait);
    }
}

impl Shell for Column {
    fn setup(&mut self, ctx: &egui::Context) {
        self.skin.apply(ctx);
        patina::emoji::install(patina_emoji_font::FONT);
        if self.skin.follows_desktop() {
            patina::appearance::watch(ctx);
        }
    }

    fn ui(&mut self, ui: &mut Ui, _output: &str) {
        self.tick(ui.ctx());
        self.thumbs.poll(ui.ctx());
        let rect = ui.max_rect();
        self.surface_h = rect.height();
        let notes = if self.showing() {
            self.shared.store().notes().to_vec()
        } else {
            Vec::new()
        };
        if notes.is_empty() {
            self.content_h = 0.0;
            return;
        }
        let mut inner = ui.new_child(
            UiBuilder::new()
                .max_rect(rect.shrink(PAD))
                .layout(Layout::top_down(Align::Min)),
        );
        let mut done: Vec<(u32, Outcome)> = Vec::new();
        let out = ScrollArea::vertical()
            .auto_shrink([false, true])
            .show(&mut inner, |ui| {
                for note in &notes {
                    let thumb = note
                        .image
                        .as_deref()
                        .or(note.file.as_deref())
                        .and_then(|p| self.thumbs.get(p));
                    let outcome = card(ui, note, thumb);
                    if outcome != Outcome::Nothing {
                        done.push((note.id, outcome));
                    }
                    ui.add_space(GAP);
                }
            });
        self.content_h = out.inner_rect.height() + 2.0 * PAD;
        let shown: Vec<&std::path::Path> = notes
            .iter()
            .filter_map(|n| n.image.as_deref().or(n.file.as_deref()))
            .collect();
        self.thumbs.keep(&shown);
        for (id, outcome) in done {
            if outcome == Outcome::Drag {
                let note = notes.iter().find(|n| n.id == id);
                if let Some(file) = note.and_then(|n| n.file.as_deref()) {
                    let thumb = note
                        .and_then(|n| n.image.as_deref().or(n.file.as_deref()))
                        .and_then(|p| self.thumbs.get(p));
                    self.drag = Some(drag_of(file, thumb, ui.ctx().pixels_per_point()));
                    self.dragged = Some(id);
                }
                continue;
            }
            // A card goes when something happened: it was dismissed, a button
            // ran, or its app's window came forward. A click that did nothing
            // leaves it where it is.
            let gone = match &outcome {
                Outcome::Nothing | Outcome::Drag => false,
                Outcome::Edit => {
                    if let Some(file) = notes.iter().find(|n| n.id == id).and_then(|n| n.file.as_deref()) {
                        edit(file);
                    }
                    false
                }
                Outcome::Dismiss => true,
                Outcome::Action(key) => {
                    self.daemon.action(id, key);
                    true
                }
                Outcome::Open => {
                    let note = self
                        .shared
                        .store()
                        .notes()
                        .iter()
                        .find(|n| n.id == id)
                        .cloned();
                    note.is_some_and(|note| {
                        if note.actions.iter().any(|(k, _)| k == "default") {
                            self.daemon.action(id, "default");
                        }
                        let window = store::window_for(&note, &self.windows);
                        if let Some(window) = window {
                            self.requests.push(ToplevelRequest::Activate(window));
                        }
                        window.is_some()
                    })
                }
            };
            if gone && self.shared.store().close(id).is_some() {
                self.daemon.closed(id, store::DISMISSED);
            }
        }
    }

    fn drags(&self) -> bool {
        true
    }

    fn take_drag(&mut self) -> Option<Drag> {
        self.drag.take()
    }

    /// An app took the file: its card has done its job, so it goes (ADR-0143). One that was
    /// refused, or called off, stays.
    fn drag_dropped(&mut self) {
        if let Some(id) = self.dragged.take()
            && self.shared.store().close(id).is_some()
        {
            self.daemon.closed(id, store::DISMISSED);
        }
    }

    fn drag_ended(&mut self) {
        self.dragged = None;
    }

    fn toplevels_changed(&mut self, toplevels: &[Toplevel]) {
        self.windows = toplevels.iter().map(|t| (t.id, t.app_id.clone())).collect();
    }

    fn take_toplevel_requests(&mut self) -> Vec<ToplevelRequest> {
        std::mem::take(&mut self.requests)
    }

    fn wanted_size(&self) -> Option<(u32, u32)> {
        Some((if self.showing() { WIDTH } else { 1 }, 0))
    }

    fn wanted_exclusive_zone(&self) -> Option<i32> {
        Some(if self.showing() { WIDTH as i32 } else { 0 })
    }

    fn wanted_input_trim(&self) -> Option<(i32, i32, i32, i32)> {
        let below = (self.surface_h - self.content_h).max(0.0);
        Some((0, 0, below as i32, 0))
    }
}

/// The most of a card's width, and the most height, a picture takes.
const PICTURE: (f32, f32) = (CARD - 24.0, 190.0);

/// One card; what was done to it, if anything. A card with a file can be dragged out.
fn card(ui: &mut Ui, note: &Note, thumb: Option<&Thumb>) -> Outcome {
    let mut outcome = Outcome::Nothing;
    // Pressed on one of its own buttons, which a drag must not start from.
    let mut on_button = false;
    let sense = if note.file.is_some() {
        Sense::click_and_drag()
    } else {
        Sense::click()
    };
    let visuals = ui.visuals().clone();
    let edge = match note.urgency {
        Urgency::Critical => visuals.error_fg_color,
        _ => visuals.widgets.noninteractive.bg_stroke.color,
    };
    let card = ui.scope_builder(UiBuilder::new().sense(sense), |ui| {
        Frame::new()
            .fill(visuals.panel_fill)
            .stroke(Stroke::new(1.0, edge))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(Margin::same(12))
            .show(ui, |ui| {
                ui.set_width(CARD - 24.0);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(&note.app)
                            .size(12.0)
                            .color(visuals.weak_text_color()),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let cross = close_button(ui);
                        on_button |= cross.is_pointer_button_down_on();
                        if cross.clicked() {
                            outcome = Outcome::Dismiss;
                        }
                    });
                });
                if !note.summary.is_empty() {
                    patina::emoji::label_with(ui, &note.summary, 15.0, RichText::strong);
                }
                if !note.body.is_empty() {
                    patina::emoji::label_with(ui, &note.body, 13.0, |t| t);
                }
                if let Some(thumb) = thumb {
                    let size = thumb.texture.size_vec2();
                    let k = (PICTURE.0 / size.x).min(PICTURE.1 / size.y).min(1.0);
                    ui.add_space(4.0);
                    ui.add(egui::Image::new((thumb.texture.id(), size * k)).corner_radius(6));
                }
                let buttons: Vec<_> = note
                    .actions
                    .iter()
                    .filter(|(k, _)| k != "default")
                    .collect();
                // A picture can be edited: Draw opens it, and the file it saves is the one dragged.
                let editable = note.file.as_deref().is_some_and(editable);
                if editable || !buttons.is_empty() {
                    ui.add_space(4.0);
                    ui.horizontal_wrapped(|ui| {
                        if editable {
                            let edit = ui.button("Edit").on_hover_text("Edit the picture in Draw");
                            on_button |= edit.is_pointer_button_down_on();
                            if edit.clicked() {
                                outcome = Outcome::Edit;
                            }
                        }
                        for (key, label) in buttons {
                            let button = ui.button(label);
                            on_button |= button.is_pointer_button_down_on();
                            if button.clicked() {
                                outcome = Outcome::Action(key.clone());
                            }
                        }
                    });
                }
            });
    });
    if outcome == Outcome::Nothing {
        if card.response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if card.response.drag_started() && !on_button && note.file.is_some() {
            outcome = Outcome::Drag;
        } else if card.response.clicked() {
            outcome = Outcome::Open;
        }
    }
    outcome
}

/// Whether a card's file is a picture Draw edits and saves: what a screenshot is.
fn editable(file: &std::path::Path) -> bool {
    file.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| ["png", "jpg", "jpeg", "bmp"].iter().any(|k| e.eq_ignore_ascii_case(k)))
}

/// Opens `file` in Draw, which saves over it, so the card shows the edit and the drag takes it.
fn edit(file: &std::path::Path) {
    use std::os::unix::process::CommandExt;
    let started = std::process::Command::new("draw")
        .arg(file)
        .stdin(std::process::Stdio::null())
        .process_group(0)
        .spawn();
    match started {
        // Waited for on a thread, so it isn't left a zombie.
        Ok(mut child) => {
            std::thread::spawn(move || child.wait());
        }
        Err(e) => eprintln!("patina-notify: can't open Draw: {e}"),
    }
}

/// The drag of a file card (ADR-0143): the file as a `file://` URI, which is how a file manager
/// offers one, so an app treats it as a file (a web page gets it as an upload), and, when it's
/// a picture, its own type too; with the thumbnail under the pointer.
fn drag_of(file: &std::path::Path, thumb: Option<&Thumb>, ppp: f32) -> Drag {
    const URI_LIST: &str = "text/uri-list";
    let picture = match file.extension().and_then(|e| e.to_str()) {
        Some(e) if e.eq_ignore_ascii_case("png") => Some("image/png"),
        Some(e) if e.eq_ignore_ascii_case("jpg") || e.eq_ignore_ascii_case("jpeg") => {
            Some("image/jpeg")
        }
        _ => None,
    };
    let mut types = vec![URI_LIST.to_string()];
    types.extend(picture.map(String::from));
    let path = file.to_path_buf();
    Drag {
        types,
        data: std::sync::Arc::new(move |mime| {
            if mime == URI_LIST {
                Some(format!("{}\r\n", store::file_uri(&path)).into_bytes())
            } else if Some(mime) == picture {
                std::fs::read(&path).ok()
            } else {
                None
            }
        }),
        icon: thumb.map(|t| drag_icon(&t.rgba, ppp)),
    }
}

/// The most, in logical pixels, the picture under the pointer takes.
const ICON: (u32, u32) = (200, 140);

/// `image` cut down to a picture for under the pointer at `ppp` pixels to the point, as
/// premultiplied ARGB, held by its middle.
fn drag_icon(image: &image::RgbaImage, ppp: f32) -> Icon {
    let scale = ppp.ceil().max(1.0) as u32;
    let k = (ICON.0 * scale) as f32 / image.width() as f32;
    let k = k
        .min((ICON.1 * scale) as f32 / image.height() as f32)
        .min(1.0);
    let (w, h) = (
        ((image.width() as f32 * k).round() as u32).max(1),
        ((image.height() as f32 * k).round() as u32).max(1),
    );
    let small = image::imageops::resize(image, w, h, image::imageops::FilterType::Triangle);
    let pixels = small
        .pixels()
        .map(|p| {
            let [r, g, b, a] = p.0.map(u32::from);
            let premultiplied = |c: u32| c * a / 255;
            a << 24 | premultiplied(r) << 16 | premultiplied(g) << 8 | premultiplied(b)
        })
        .collect();
    Icon {
        size: (w, h),
        scale: scale as i32,
        pixels,
        hotspot: ((w / 2) as i32, (h / 2) as i32),
    }
}

/// The ✕ in a card's top right: two strokes, as the dialogs draw theirs, so no
/// font has to have the glyph.
fn close_button(ui: &mut Ui) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
    let colour = if response.hovered() {
        ui.visuals().text_color()
    } else {
        ui.visuals().weak_text_color()
    };
    let (centre, d) = (rect.center(), 4.5);
    let stroke = Stroke::new(1.5, colour);
    ui.painter()
        .line_segment([centre + vec2(-d, -d), centre + vec2(d, d)], stroke);
    ui.painter()
        .line_segment([centre + vec2(-d, d), centre + vec2(d, -d)], stroke);
    response
}

pub fn run() -> Result<(), String> {
    let shared = Shared::new();
    let (wake, woken) = channel::channel::<()>();
    let fullscreen = Arc::new(AtomicBool::new(false));
    fullscreen::watch(fullscreen.clone(), wake.clone());
    let thumbs = Thumbs::new(wake.clone());
    let daemon = Daemon::serve(shared.clone(), wake)?;
    let column = Column {
        skin: piece::skin("patina-notify"),
        shared,
        daemon,
        windows: Vec::new(),
        requests: Vec::new(),
        thumbs,
        stamps: Stamps::default(),
        files_checked: None,
        drag: None,
        dragged: None,
        enabled: true,
        hide_in_fullscreen: true,
        fullscreen,
        config_seen: None,
        config_checked: None,
        surface_h: 0.0,
        content_h: 0.0,
    };
    let options = Options {
        namespace: "patina-notify".into(),
        layer: Layer::Top,
        anchor: Anchor::TOP | Anchor::RIGHT | Anchor::BOTTOM,
        size: (1, 0),
        exclusive_zone: 0,
        margin: (0, 0, 0, 0),
        keyboard: KeyboardInteractivity::None,
        per_output: false,
        toplevels: true,
        click_through: false,
        lock: false,
        window_fallback: false,
    };
    let (host, event_loop) = Host::connect(options, column)?;
    event_loop
        .handle()
        .insert_source(woken, |event, _, host| {
            if let channel::Event::Msg(()) = event {
                host.redraw_all();
            }
        })
        .map_err(|e| e.to_string())?;
    host.run(event_loop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_under_the_pointer_is_cut_down_and_premultiplied() {
        let image = image::RgbaImage::from_pixel(1000, 500, image::Rgba([200, 100, 50, 255]));
        let icon = drag_icon(&image, 2.0);
        assert_eq!(icon.scale, 2);
        assert_eq!(icon.size, (400, 200), "200 by 100 points at 2x");
        assert_eq!(icon.pixels.len(), 400 * 200);
        assert_eq!(icon.pixels[0], 0xFFC8_6432);
        assert_eq!(icon.hotspot, (200, 100));
        let clear = image::RgbaImage::from_pixel(10, 10, image::Rgba([255, 255, 255, 0]));
        assert_eq!(drag_icon(&clear, 1.0).pixels[0], 0, "clear stays clear");
        let small = image::RgbaImage::from_pixel(20, 10, image::Rgba([0, 0, 0, 255]));
        assert_eq!(
            drag_icon(&small, 1.0).size,
            (20, 10),
            "a small one isn't blown up"
        );
    }

    #[test]
    fn a_dragged_picture_is_offered_as_a_file_first_and_as_itself() {
        let drag = drag_of(std::path::Path::new("/tmp/a b.png"), None, 1.0);
        assert_eq!(drag.types, ["text/uri-list", "image/png"]);
        let uri = (drag.data)("text/uri-list").unwrap();
        assert_eq!(uri, b"file:///tmp/a%20b.png\r\n");
        assert!((drag.data)("text/plain").is_none());
        let other = drag_of(std::path::Path::new("/tmp/notes.txt"), None, 1.0);
        assert_eq!(other.types, ["text/uri-list"]);
    }

    use std::path::Path;

    fn note(file: Option<&str>, actions: &[(&str, &str)]) -> Note {
        Note {
            id: 1,
            app: "Screenshot".into(),
            desktop_entry: String::new(),
            image: file.map(Into::into),
            file: file.map(Into::into),
            summary: "Screenshot saved".into(),
            body: "Screenshot 2026-11-19 at 22.16.56.png".into(),
            actions: actions.iter().map(|(k, l)| (k.to_string(), l.to_string())).collect(),
            urgency: Urgency::Normal,
            expires: None,
        }
    }

    #[test]
    fn the_pictures_draw_saves_can_be_edited_and_nothing_else() {
        for yes in ["a.png", "A.PNG", "x.jpg", "x.jpeg", "x.bmp", "/home/me/Pictures/Screenshots/Shot 1.png"] {
            assert!(editable(Path::new(yes)), "{yes}");
        }
        for no in ["a.gif", "a.webp", "a.svg", "a.txt", "a", "a.mp4"] {
            assert!(!editable(Path::new(no)), "{no}");
        }
    }

    /// Draws the cards, to see them: `PATINA_NOTIFY_PREVIEW=/tmp/p cargo test -p patina-notify
    /// preview -- --ignored`.
    #[test]
    #[ignore]
    fn preview_the_cards() {
        let dir = std::path::PathBuf::from(std::env::var("PATINA_NOTIFY_PREVIEW").unwrap());
        std::fs::create_dir_all(&dir).unwrap();
        let (w, h) = (CARD as usize + 24, 520usize);
        let ctx = egui::Context::default();
        let skin = piece::skin("patina-notify");
        skin.apply(&ctx);
        patina::emoji::install(patina_emoji_font::FONT);
        let picture = image::RgbaImage::from_fn(640, 360, |x, y| {
            image::Rgba([(x / 3) as u8, (y / 2) as u8, 160, 255])
        });
        let colours = egui::ColorImage::from_rgba_unmultiplied([640, 360], picture.as_raw());
        let thumb = Thumb {
            texture: ctx.load_texture("preview", colours, Default::default()),
            rgba: Arc::new(picture),
        };
        let cards = [
            (note(Some("/home/me/Pictures/Screenshots/Screenshot 2026-11-19 at 22.16.56.png"), &[]), true),
            (note(None, &[("default", ""), ("reply", "Reply")]), false),
        ];
        let mut painter = patina_shell::raster::Painter::default();
        let mut pixels = vec![0u32; w * h];
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::pos2(0.0, 0.0), vec2(w as f32, h as f32))),
                max_texture_side: Some(8192),
                ..Default::default()
            };
            let out = ctx.run_ui(input, |ui| {
                ui.add_space(12.0);
                for (n, with_thumb) in &cards {
                    ui.horizontal(|ui| {
                        ui.add_space(12.0);
                        ui.vertical(|ui| {
                            card(ui, n, with_thumb.then_some(&thumb));
                        });
                    });
                    ui.add_space(GAP);
                }
            });
            let primitives = ctx.tessellate(out.shapes, out.pixels_per_point);
            painter.paint(&mut pixels, w, h, out.pixels_per_point, &primitives, &out.textures_delta);
        }
        let rgba: Vec<u8> = pixels
            .iter()
            .flat_map(|p| {
                let [b, g, r, a] = p.to_le_bytes();
                [r, g, b, a]
            })
            .collect();
        image::save_buffer(dir.join("cards.png"), &rgba, w as u32, h as u32, image::ColorType::Rgba8).unwrap();
    }
}
