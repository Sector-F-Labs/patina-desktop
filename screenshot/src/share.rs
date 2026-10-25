//! What an app may see when it asks to share the screen (ADR-0092): the
//! chooser xdg-desktop-portal-wlr runs (`chooser_type=dmenu`). It reads the
//! choices on stdin, a line each, `Monitor: NAME …` for a screen and
//! `Window: TITLE (IDENTIFIER)` for a window (ext-foreign-toplevel-list's
//! identifier, not shown), shows them in a panel in the middle of the
//! screen, and prints the line chosen; nothing if it's closed with Escape,
//! which the portal takes as a no.

use std::cell::RefCell;
use std::io::BufRead;
use std::rc::Rc;

use egui::{
    Align2, CornerRadius, Key, Rect, ScrollArea, Sense, Stroke, StrokeKind, TextStyle, Ui, pos2,
    vec2,
};
use patina::skin::Skin;
use patina_shell::piece;
use patina_shell::{Anchor, Host, KeyboardInteractivity, Layer, Options, Shell};

const WIDTH: u32 = 460;
const ROW: f32 = 38.0;
const HEADER: f32 = 52.0;
const PAD: f32 = 10.0;
/// Rows shown before it scrolls.
const MOST_ROWS: usize = 10;

/// One line of the portal's.
#[derive(Debug, Clone, PartialEq)]
pub struct Source {
    /// The line as it came, printed back if it's chosen.
    pub line: String,
    pub kind: Kind,
    /// What's shown: the screen's name, or the window's title.
    pub name: String,
    /// Beside it, fainter: the screen's make and model.
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Screen,
    Window,
}

/// A line of the portal's, or `None` for a blank one.
pub fn parse(line: &str) -> Option<Source> {
    let line = line.trim_end_matches(['\n', '\r']);
    if line.trim().is_empty() {
        return None;
    }
    let source = |kind, name: &str, detail: &str| Source {
        line: line.to_string(),
        kind,
        name: name.trim().to_string(),
        detail: detail.trim().to_string(),
    };
    if let Some(rest) = line.strip_prefix("Monitor: ") {
        let (name, detail) = rest.split_once(' ').unwrap_or((rest, ""));
        return Some(source(Kind::Screen, name, detail));
    }
    if let Some(rest) = line.strip_prefix("Window: ") {
        // "Title (identifier)": the identifier means nothing to anyone.
        let title = match rest.rfind(" (") {
            Some(open) if rest.ends_with(')') => &rest[..open],
            _ => rest,
        };
        return Some(source(Kind::Window, title, ""));
    }
    Some(source(Kind::Screen, line, ""))
}

/// The panel's height for `n` sources.
fn height(n: usize) -> u32 {
    (HEADER + ROW * n.clamp(1, MOST_ROWS) as f32 + PAD * 2.0) as u32
}

pub struct Picker {
    skin: Skin,
    sources: Vec<Source>,
    current: usize,
    chosen: Rc<RefCell<Option<String>>>,
    had_focus: bool,
    done: bool,
}

impl Picker {
    pub fn new(skin: Skin, sources: Vec<Source>, chosen: Rc<RefCell<Option<String>>>) -> Self {
        Self {
            skin,
            sources,
            current: 0,
            chosen,
            had_focus: false,
            done: false,
        }
    }

    fn choose(&mut self, i: usize) {
        if let Some(source) = self.sources.get(i) {
            *self.chosen.borrow_mut() = Some(source.line.clone());
            self.done = true;
        }
    }

    fn keys(&mut self, ui: &Ui) {
        let n = self.sources.len().max(1);
        let (escape, up, down, enter) = ui.input(|i| {
            (
                i.key_pressed(Key::Escape),
                i.key_pressed(Key::ArrowUp) || (i.key_pressed(Key::Tab) && i.modifiers.shift),
                i.key_pressed(Key::ArrowDown) || (i.key_pressed(Key::Tab) && !i.modifiers.shift),
                i.key_pressed(Key::Enter),
            )
        });
        if escape {
            self.done = true;
        } else if enter {
            self.choose(self.current);
        } else if up {
            self.current = (self.current + n - 1) % n;
        } else if down {
            self.current = (self.current + 1) % n;
        }
    }

    /// Draws it in `ui`'s whole rect.
    pub fn draw(&mut self, ui: &mut Ui) {
        let rect = ui.max_rect();
        let v = ui.visuals().clone();
        ui.painter().rect(
            rect,
            v.window_corner_radius,
            v.panel_fill,
            v.window_stroke,
            StrokeKind::Inside,
        );
        let inner = rect.shrink(PAD);
        ui.painter().text(
            pos2(inner.left() + 6.0, inner.top() + 14.0),
            Align2::LEFT_CENTER,
            "Share your screen",
            TextStyle::Heading.resolve(ui.style()),
            v.strong_text_color(),
        );
        ui.painter().text(
            pos2(inner.left() + 6.0, inner.top() + 36.0),
            Align2::LEFT_CENTER,
            "Choose what the app may see",
            TextStyle::Small.resolve(ui.style()),
            v.weak_text_color(),
        );
        if self.sources.is_empty() {
            ui.painter().text(
                pos2(inner.center().x, inner.top() + HEADER + ROW / 2.0),
                Align2::CENTER_CENTER,
                "Nothing to share",
                TextStyle::Body.resolve(ui.style()),
                v.weak_text_color(),
            );
            return;
        }
        let list = Rect::from_min_max(pos2(inner.left(), inner.top() + HEADER), inner.max);
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(list));
        let mut clicked = None;
        ScrollArea::vertical().show(&mut ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for (i, source) in self.sources.iter().enumerate() {
                let (row, response) =
                    ui.allocate_exact_size(vec2(list.width(), ROW), Sense::click());
                if response.hovered() {
                    self.current = i;
                }
                if self.current == i {
                    ui.painter().rect_filled(
                        row,
                        CornerRadius::same(8),
                        v.widgets.hovered.weak_bg_fill,
                    );
                    if ui.input(|i| i.key_pressed(Key::ArrowUp) || i.key_pressed(Key::ArrowDown)) {
                        response.scroll_to_me(None);
                    }
                }
                let icon = Rect::from_center_size(
                    pos2(row.left() + 22.0, row.center().y),
                    vec2(20.0, 14.0),
                );
                draw_icon(ui, source.kind, icon, v.text_color(), v.panel_fill);
                let galley = ui.painter().layout_no_wrap(
                    source.name.clone(),
                    TextStyle::Body.resolve(ui.style()),
                    v.text_color(),
                );
                let text_at = pos2(row.left() + 44.0, row.center().y - galley.size().y / 2.0);
                let name_width = galley.size().x;
                ui.painter()
                    .with_clip_rect(row)
                    .galley(text_at, galley, v.text_color());
                if !source.detail.is_empty() {
                    ui.painter().with_clip_rect(row).text(
                        pos2(text_at.x + name_width + 8.0, row.center().y),
                        Align2::LEFT_CENTER,
                        &source.detail,
                        TextStyle::Small.resolve(ui.style()),
                        v.weak_text_color(),
                    );
                }
                if response.clicked() {
                    clicked = Some(i);
                }
            }
        });
        if let Some(i) = clicked {
            self.choose(i);
        }
    }
}

/// A monitor, or a window with its title bar, in `r`.
fn draw_icon(ui: &Ui, kind: Kind, r: Rect, colour: egui::Color32, fill: egui::Color32) {
    let painter = ui.painter();
    let stroke = Stroke::new(1.5, colour);
    match kind {
        Kind::Screen => {
            let screen = Rect::from_min_max(r.min, pos2(r.max.x, r.max.y - 3.0));
            painter.rect_stroke(screen, CornerRadius::same(2), stroke, StrokeKind::Inside);
            painter.line_segment(
                [
                    pos2(r.center().x - 4.0, r.bottom() - 0.75),
                    pos2(r.center().x + 4.0, r.bottom() - 0.75),
                ],
                stroke,
            );
        }
        Kind::Window => {
            painter.rect_filled(r, CornerRadius::same(2), fill);
            painter.rect_filled(
                Rect::from_min_size(r.min, vec2(r.width(), 4.0)),
                CornerRadius {
                    nw: 2,
                    ne: 2,
                    sw: 0,
                    se: 0,
                },
                colour,
            );
            painter.rect_stroke(r, CornerRadius::same(2), stroke, StrokeKind::Inside);
        }
    }
}

impl Shell for Picker {
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
        }
        self.keys(ui);
        if self.done {
            return;
        }
        self.draw(ui);
    }

    fn finished(&self) -> bool {
        self.done
    }
}

/// Reads the choices, shows them, and returns the line chosen.
pub fn run() -> Result<Option<String>, String> {
    let sources: Vec<Source> = std::io::stdin()
        .lock()
        .lines()
        .map_while(Result::ok)
        .filter_map(|line| parse(&line))
        .collect();
    let chosen = Rc::new(RefCell::new(None));
    let options = Options {
        namespace: "patina-screenshot-share".into(),
        layer: Layer::Overlay,
        anchor: Anchor::empty(),
        size: (WIDTH, height(sources.len())),
        exclusive_zone: 0,
        margin: (0, 0, 0, 0),
        keyboard: KeyboardInteractivity::Exclusive,
        per_output: false,
        toplevels: false,
        click_through: false,
        lock: false,
        window_fallback: false,
    };
    let picker = Picker::new(piece::skin("patina-screenshot"), sources, chosen.clone());
    let (host, event_loop) = Host::connect(options, picker)?;
    host.run(event_loop)?;
    Ok(chosen.take())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn the_portals_lines_are_read() {
        let screen = parse("Monitor: eDP-1 Sharp Corporation 0x1515\n").unwrap();
        assert_eq!(screen.kind, Kind::Screen);
        assert_eq!(screen.name, "eDP-1");
        assert_eq!(screen.detail, "Sharp Corporation 0x1515");
        assert_eq!(screen.line, "Monitor: eDP-1 Sharp Corporation 0x1515");
        let line = "Window: Notes (draft) (KojRFTDuanTNCsSJVTG31654M9vM3W)";
        let window = parse(line).unwrap();
        assert_eq!(window.kind, Kind::Window);
        assert_eq!(window.name, "Notes (draft)");
        assert_eq!(window.detail, "");
        assert_eq!(window.line, line, "printed back as it came");
        assert_eq!(parse("Window: bare").unwrap().name, "bare");
        assert_eq!(parse("  \n"), None);
    }

    #[test]
    fn it_grows_with_the_list_then_scrolls() {
        assert!(height(3) > height(1));
        assert_eq!(height(0), height(1));
        assert_eq!(height(50), height(MOST_ROWS));
    }

    /// Draws it with a screen and some windows, in software, as a PNG in
    /// the folder PATINA_SCREENSHOT_PREVIEW names.
    #[test]
    #[ignore]
    fn preview_the_picker() {
        let dir = PathBuf::from(std::env::var("PATINA_SCREENSHOT_PREVIEW").unwrap());
        let lines = [
            "Monitor: eDP-1 Sharp Corporation 0x1515",
            "Window: Inbox — Mail (aG3kL0pQ)",
            "Window: patina-desktop — Teleprinter (Zr8sT2vW)",
            "Window: Sector F Labs — Mozilla Firefox (Lm4nB7cX)",
        ];
        let sources: Vec<Source> = lines.iter().filter_map(|l| parse(l)).collect();
        let (w, h) = (WIDTH as usize * 2, height(sources.len()) as usize * 2);
        let ctx = egui::Context::default();
        let mut picker = Picker::new(Skin::default_skin(), sources, Rc::default());
        picker.skin.apply(&ctx);
        let mut painter = patina_shell::raster::Painter::default();
        let mut pixels = vec![0u32; w * h];
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    pos2(0.0, 0.0),
                    vec2(w as f32 / 2.0, h as f32 / 2.0),
                )),
                max_texture_side: Some(8192),
                ..Default::default()
            };
            ctx.set_pixels_per_point(2.0);
            let out = ctx.run_ui(input, |ui| picker.draw(ui));
            let primitives = ctx.tessellate(out.shapes, out.pixels_per_point);
            pixels.fill(0);
            painter.paint(&mut pixels, w, h, 2.0, &primitives, &out.textures_delta);
        }
        let rgba: Vec<u8> = pixels
            .iter()
            .flat_map(|p| {
                let [b, g, r, a] = p.to_le_bytes();
                [r, g, b, a]
            })
            .collect();
        image::save_buffer(
            dir.join("share.png"),
            &rgba,
            w as u32,
            h as u32,
            image::ColorType::Rgba8,
        )
        .unwrap();
    }
}
