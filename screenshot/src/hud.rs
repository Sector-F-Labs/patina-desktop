//! The pill (ADR-0091): a close button, then the screen, the window and a
//! selection side by side, each an icon over its name. A click or Enter on
//! one chooses it; arrows, Tab and 1 to 3 move between them; Escape, or the
//! keyboard going elsewhere, closes it with nothing chosen.

use std::cell::Cell;
use std::rc::Rc;

use egui::{
    Align2, Color32, CornerRadius, Key, Rect, Sense, Shape, Stroke, StrokeKind, TextStyle, Ui,
    pos2, vec2,
};
use patina::skin::Skin;
use patina_shell::Shell;

/// The pill, in logical pixels.
pub const SIZE: (u32, u32) = (340, 76);
/// The space around the tiles, and the close button's width; the tiles
/// share the rest.
const PAD: f32 = 8.0;
const CLOSE: f32 = 36.0;
const FADE_IN: f64 = 0.12;

/// What can be taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Screen,
    Window,
    Selection,
}

impl Choice {
    pub const ALL: [Choice; 3] = [Choice::Screen, Choice::Window, Choice::Selection];

    /// What's printed for the compositor.
    pub fn word(self) -> &'static str {
        match self {
            Choice::Screen => "screen",
            Choice::Window => "window",
            Choice::Selection => "selection",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Choice::Screen => "Screen",
            Choice::Window => "Window",
            Choice::Selection => "Selection",
        }
    }
}

pub struct Hud {
    skin: Skin,
    /// Whether there's a focused window to take.
    window: bool,
    /// The tile the keyboard is on, as an index into `Choice::ALL`.
    current: usize,
    chosen: Rc<Cell<Option<Choice>>>,
    shown_at: Option<f64>,
    had_focus: bool,
    done: bool,
}

impl Hud {
    pub fn new(skin: Skin, window: bool, chosen: Rc<Cell<Option<Choice>>>) -> Self {
        Self {
            skin,
            window,
            current: 0,
            chosen,
            shown_at: None,
            had_focus: false,
            done: false,
        }
    }

    fn offered(&self, choice: Choice) -> bool {
        choice != Choice::Window || self.window
    }

    /// The next offered tile `by` steps along, wrapping round.
    fn step(&self, by: isize) -> usize {
        let n = Choice::ALL.len() as isize;
        let mut i = self.current as isize;
        loop {
            i = (i + by).rem_euclid(n);
            if self.offered(Choice::ALL[i as usize]) {
                return i as usize;
            }
        }
    }

    fn choose(&mut self, choice: Choice) {
        if self.offered(choice) {
            self.chosen.set(Some(choice));
            self.done = true;
        }
    }

    fn keys(&mut self, ui: &Ui) {
        let (escape, back, forward, enter, digit) = ui.input(|i| {
            let digit = [Key::Num1, Key::Num2, Key::Num3]
                .iter()
                .position(|k| i.key_pressed(*k));
            (
                i.key_pressed(Key::Escape),
                i.key_pressed(Key::ArrowLeft) || (i.key_pressed(Key::Tab) && i.modifiers.shift),
                i.key_pressed(Key::ArrowRight) || (i.key_pressed(Key::Tab) && !i.modifiers.shift),
                i.key_pressed(Key::Enter) || i.key_pressed(Key::Space),
                digit,
            )
        });
        if escape {
            self.done = true;
        } else if let Some(n) = digit {
            self.choose(Choice::ALL[n]);
        } else if enter {
            self.choose(Choice::ALL[self.current]);
        } else if back {
            self.current = self.step(-1);
        } else if forward {
            self.current = self.step(1);
        }
    }

    /// Draws it in `ui`'s whole rect.
    pub fn draw(&mut self, ui: &mut Ui) {
        let rect = ui.max_rect();
        let v = ui.visuals().clone();
        ui.painter().rect(
            rect,
            CornerRadius::same(22),
            v.panel_fill,
            v.window_stroke,
            StrokeKind::Inside,
        );
        let inner = rect.shrink(PAD);

        // Close, at the left, then a line between it and the choices.
        let close = Rect::from_min_size(inner.min, vec2(CLOSE, inner.height()));
        let response = ui.interact(close, ui.id().with("close"), Sense::click());
        let mark = Rect::from_center_size(close.center(), vec2(24.0, 24.0));
        if response.hovered() {
            ui.painter()
                .circle_filled(mark.center(), 12.0, v.widgets.hovered.weak_bg_fill);
        }
        let x = Rect::from_center_size(mark.center(), vec2(9.0, 9.0));
        let stroke = Stroke::new(1.5, v.weak_text_color());
        ui.painter()
            .line_segment([x.left_top(), x.right_bottom()], stroke);
        ui.painter()
            .line_segment([x.right_top(), x.left_bottom()], stroke);
        if response.clicked() {
            self.done = true;
        }
        let divider = close.right() + PAD / 2.0;
        ui.painter().vline(
            divider,
            inner.y_range().shrink(10.0),
            v.widgets.noninteractive.bg_stroke,
        );

        let mut left = divider + PAD / 2.0;
        let width = (inner.right() - left) / Choice::ALL.len() as f32;
        for (i, choice) in Choice::ALL.into_iter().enumerate() {
            let tile = Rect::from_min_size(pos2(left, inner.top()), vec2(width, inner.height()));
            left += width;
            let offered = self.offered(choice);
            let sense = if offered {
                Sense::click()
            } else {
                Sense::hover()
            };
            let response = ui.interact(tile, ui.id().with(choice.word()), sense);
            if response.hovered() && offered {
                self.current = i;
            }
            if self.current == i && offered {
                ui.painter().rect_filled(
                    tile,
                    CornerRadius::same(14),
                    v.widgets.hovered.weak_bg_fill,
                );
            }
            let colour = if offered {
                v.text_color()
            } else {
                v.weak_text_color().gamma_multiply(0.6)
            };
            let icon =
                Rect::from_center_size(pos2(tile.center().x, tile.top() + 22.0), vec2(26.0, 18.0));
            draw_icon(ui, choice, icon, colour, v.panel_fill);
            ui.painter().text(
                pos2(tile.center().x, tile.bottom() - 13.0),
                Align2::CENTER_CENTER,
                choice.label(),
                TextStyle::Body.resolve(ui.style()),
                colour,
            );
            if response.clicked() {
                self.choose(choice);
            }
        }
    }
}

/// `choice`'s icon in `r`: a monitor, a window with its title bar, or a
/// dashed rectangle.
fn draw_icon(ui: &Ui, choice: Choice, r: Rect, colour: Color32, fill: Color32) {
    let painter = ui.painter();
    let stroke = Stroke::new(1.5, colour);
    match choice {
        Choice::Screen => {
            let screen = Rect::from_min_max(r.min, pos2(r.max.x, r.max.y - 4.0));
            painter.rect_stroke(screen, CornerRadius::same(2), stroke, StrokeKind::Inside);
            let foot = r.bottom() - 0.75;
            painter.line_segment(
                [
                    pos2(r.center().x - 5.0, foot),
                    pos2(r.center().x + 5.0, foot),
                ],
                stroke,
            );
        }
        Choice::Window => {
            painter.rect_filled(r, CornerRadius::same(2), fill);
            painter.rect_filled(
                Rect::from_min_size(r.min, vec2(r.width(), 5.0)),
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
        Choice::Selection => {
            let r = r.shrink(0.75);
            let corners = [
                r.left_top(),
                r.right_top(),
                r.right_bottom(),
                r.left_bottom(),
                r.left_top(),
            ];
            painter.extend(Shape::dashed_line(&corners, stroke, 3.0, 2.5));
        }
    }
}

impl Shell for Hud {
    fn setup(&mut self, ctx: &egui::Context) {
        self.skin.apply(ctx);
        if self.skin.follows_desktop() {
            patina::appearance::watch(ctx);
        }
    }

    fn ui(&mut self, ui: &mut Ui, _: &str) {
        let now = ui.input(|i| i.time);
        let shown_at = *self.shown_at.get_or_insert(now);
        if ui.input(|i| i.focused) {
            self.had_focus = true;
        } else if self.had_focus {
            self.done = true;
        }
        self.keys(ui);
        if self.done {
            return;
        }
        let opacity = ((now - shown_at) / FADE_IN).clamp(0.0, 1.0) as f32;
        if opacity < 1.0 {
            ui.ctx().request_repaint();
        }
        ui.set_opacity(opacity);
        self.draw(ui);
    }

    fn finished(&self) -> bool {
        self.done
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn hud(window: bool) -> Hud {
        Hud::new(Skin::default_skin(), window, Rc::default())
    }

    #[test]
    fn the_keys_step_past_a_missing_window() {
        let mut h = hud(false);
        assert_eq!(h.step(1), 2);
        h.current = 2;
        assert_eq!(h.step(1), 0);
        assert_eq!(h.step(-1), 0);
        assert_eq!(hud(true).step(1), 1);
    }

    #[test]
    fn a_missing_window_cant_be_chosen() {
        let mut h = hud(false);
        h.choose(Choice::Window);
        assert_eq!((h.chosen.get(), h.done), (None, false));
        h.choose(Choice::Selection);
        assert_eq!((h.chosen.get(), h.done), (Some(Choice::Selection), true));
    }

    /// Draws it in each skin, in software, as PNGs in the folder
    /// PATINA_SCREENSHOT_PREVIEW names, to look at.
    #[test]
    #[ignore]
    fn preview_the_hud() {
        let dir = PathBuf::from(std::env::var("PATINA_SCREENSHOT_PREVIEW").unwrap());
        let scale = 2.0;
        let (w, h) = (SIZE.0 as usize * 2, SIZE.1 as usize * 2);
        let (skins, _) = patina::skin::load_all(patina::skin::BUILT_IN, &[]);
        let each = skins
            .into_iter()
            .map(|skin| (skin.name.clone(), skin, true));
        let no_window = ("no-window".to_string(), Skin::default_skin(), false);
        for (name, skin, window) in each.chain([no_window]) {
            let ctx = egui::Context::default();
            let mut hud = Hud::new(skin, window, Rc::default());
            hud.skin.apply(&ctx);
            let mut painter = patina_shell::raster::Painter::default();
            let mut pixels = vec![0u32; w * h];
            for _ in 0..3 {
                let input = egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        pos2(0.0, 0.0),
                        vec2(SIZE.0 as f32, SIZE.1 as f32),
                    )),
                    max_texture_side: Some(8192),
                    ..Default::default()
                };
                ctx.set_pixels_per_point(scale);
                let out = ctx.run_ui(input, |ui| hud.draw(ui));
                let primitives = ctx.tessellate(out.shapes, out.pixels_per_point);
                pixels.fill(0);
                painter.paint(&mut pixels, w, h, scale, &primitives, &out.textures_delta);
            }
            let rgba: Vec<u8> = pixels
                .iter()
                .flat_map(|p| {
                    let [b, g, r, a] = p.to_le_bytes();
                    [r, g, b, a]
                })
                .collect();
            image::save_buffer(
                dir.join(format!("{name}.png")),
                &rgba,
                w as u32,
                h as u32,
                image::ColorType::Rgba8,
            )
            .unwrap();
        }
    }
}
