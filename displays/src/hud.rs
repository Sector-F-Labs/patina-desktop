//! The pill (ADR-0121): the monitors named along the top, then Extend,
//! Mirror and a third that does nothing, each an icon over its name, the one
//! in use marked. With a monitor waiting to be chosen for, the third is "Do
//! nothing" and leaves it off; otherwise it's "Primary only", which turns the
//! others off. A click or Enter chooses; arrows and Tab move, 1 to 3 choose;
//! Escape, or the keyboard going elsewhere, closes it with nothing chosen.

use std::cell::Cell;
use std::rc::Rc;

use egui::{
    Align2, Color32, CornerRadius, Key, Rect, Sense, Stroke, StrokeKind, TextStyle, Ui, pos2, vec2,
};
use patina::skin::Skin;
use patina_shell::Shell;
use patina_system::displays::{Mode, Monitor};

/// The pill, in logical pixels.
pub const SIZE: (u32, u32) = (560, 120);
const PAD: f32 = 8.0;
const CLOSE: f32 = 36.0;
const FADE_IN: f64 = 0.12;
const CHOICES: [Mode; 3] = [Mode::Extend, Mode::Mirror, Mode::Off];

/// `mode`'s name on its tile; with `waiting` monitors, the third does nothing.
fn label(mode: Mode, waiting: bool) -> &'static str {
    match (mode, waiting) {
        (Mode::Extend, _) => "Extend",
        (Mode::Mirror, _) => "Mirror",
        (Mode::Off, true) => "Do nothing",
        (Mode::Off, false) => "Primary only",
    }
}

/// The line of monitor names: "Built-in · LG HDR WQHD", or what to do when
/// there's only one.
pub fn summary(monitors: &[Monitor]) -> String {
    if monitors.len() < 2 {
        return "Only one display is connected".into();
    }
    monitors
        .iter()
        .map(|m| {
            let off = if m.active { "" } else { ", off" };
            format!("{} ({}×{}{off})", m.label(), m.width, m.height)
        })
        .collect::<Vec<_>>()
        .join("  ·  ")
}

pub struct Hud {
    skin: Skin,
    summary: String,
    /// With one monitor there's nothing to extend onto or mirror.
    usable: bool,
    /// A monitor is connected and off, waiting to be chosen for.
    waiting: bool,
    in_use: Mode,
    /// The tile the keyboard is on, as an index into `CHOICES`.
    current: usize,
    chosen: Rc<Cell<Option<Mode>>>,
    shown_at: Option<f64>,
    had_focus: bool,
    done: bool,
}

impl Hud {
    pub fn new(
        skin: Skin,
        monitors: &[Monitor],
        in_use: Mode,
        chosen: Rc<Cell<Option<Mode>>>,
    ) -> Self {
        Self {
            skin,
            summary: summary(monitors),
            usable: monitors.len() > 1,
            waiting: monitors.iter().any(|m| !m.active),
            in_use,
            // Extending onto a monitor just plugged in; else the other of
            // extend and mirror, the likelier reason to have asked.
            current: if monitors.iter().any(|m| !m.active) {
                0
            } else {
                CHOICES[..2].iter().position(|m| *m != in_use).unwrap_or(0)
            },
            chosen,
            shown_at: None,
            had_focus: false,
            done: false,
        }
    }

    fn choose(&mut self, mode: Mode) {
        if !self.usable {
            return;
        }
        // Doing nothing about a monitor waiting is just closing.
        if !(mode == Mode::Off && self.waiting) {
            self.chosen.set(Some(mode));
        }
        self.done = true;
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
            self.choose(CHOICES[n]);
        } else if enter {
            self.choose(CHOICES[self.current]);
        } else if back {
            self.current = (self.current + CHOICES.len() - 1) % CHOICES.len();
        } else if forward {
            self.current = (self.current + 1) % CHOICES.len();
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

        // Close, at the left, then a line between it and the rest.
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

        let area = Rect::from_min_max(pos2(divider + PAD / 2.0, inner.top()), inner.max);
        ui.painter().text(
            pos2(area.center().x, area.top() + 10.0),
            Align2::CENTER_CENTER,
            &self.summary,
            TextStyle::Small.resolve(ui.style()),
            v.weak_text_color(),
        );
        let tiles = Rect::from_min_max(pos2(area.left(), area.top() + 24.0), area.max);
        let width = tiles.width() / CHOICES.len() as f32;
        for (i, mode) in CHOICES.into_iter().enumerate() {
            let tile = Rect::from_min_size(
                pos2(tiles.left() + width * i as f32, tiles.top()),
                vec2(width, tiles.height()),
            );
            let sense = if self.usable {
                Sense::click()
            } else {
                Sense::hover()
            };
            let response = ui.interact(tile, ui.id().with(mode.name()), sense);
            if response.hovered() && self.usable {
                self.current = i;
            }
            if self.current == i && self.usable {
                ui.painter().rect_filled(
                    tile.shrink(2.0),
                    CornerRadius::same(14),
                    v.widgets.hovered.weak_bg_fill,
                );
            }
            let colour = if self.usable {
                v.text_color()
            } else {
                v.weak_text_color().gamma_multiply(0.6)
            };
            let icon =
                Rect::from_center_size(pos2(tile.center().x, tile.top() + 18.0), vec2(34.0, 18.0));
            draw_icon(ui, mode, icon, colour);
            // A dot beside the name marks the one in use.
            let shown = label(mode, self.waiting);
            let name = if mode == self.in_use && !self.waiting {
                format!("● {shown}")
            } else {
                shown.to_string()
            };
            ui.painter().text(
                pos2(tile.center().x, tile.bottom() - 12.0),
                Align2::CENTER_CENTER,
                name,
                TextStyle::Body.resolve(ui.style()),
                colour,
            );
            if response.clicked() {
                self.choose(mode);
            }
        }
    }
}

/// `mode`'s icon in `r`: two screens side by side, one over the other, or
/// one alone beside a dashed outline of where the other isn't.
fn draw_icon(ui: &Ui, mode: Mode, r: Rect, colour: Color32) {
    let painter = ui.painter();
    let stroke = Stroke::new(1.5, colour);
    let screen =
        |at: Rect| painter.rect_stroke(at, CornerRadius::same(2), stroke, StrokeKind::Inside);
    match mode {
        Mode::Extend => {
            let w = (r.width() - 4.0) / 2.0;
            screen(Rect::from_min_size(r.min, vec2(w, r.height())));
            screen(Rect::from_min_size(
                pos2(r.max.x - w, r.min.y),
                vec2(w, r.height()),
            ));
        }
        Mode::Off => {
            let w = (r.width() - 4.0) / 2.0;
            screen(Rect::from_min_size(r.min, vec2(w, r.height())));
            let ghost =
                Rect::from_min_size(pos2(r.max.x - w, r.min.y), vec2(w, r.height())).shrink(0.75);
            let corners = [
                ghost.left_top(),
                ghost.right_top(),
                ghost.right_bottom(),
                ghost.left_bottom(),
                ghost.left_top(),
            ];
            painter.extend(egui::Shape::dashed_line(&corners, stroke, 3.0, 2.5));
        }
        Mode::Mirror => {
            let (w, h) = (r.width() * 0.7, r.height() * 0.8);
            screen(Rect::from_min_size(
                pos2(r.min.x + r.width() * 0.3, r.min.y),
                vec2(w, h),
            ));
            screen(Rect::from_min_size(
                pos2(r.min.x, r.min.y + r.height() * 0.2),
                vec2(w, h),
            ));
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
    use super::*;

    fn monitor(name: &str, model: &str) -> Monitor {
        Monitor {
            name: name.into(),
            id: name.into(),
            make: String::new(),
            model: model.into(),
            width: 1920,
            height: 1200,
            x: 0,
            y: 0,
            logical_width: 1600,
            logical_height: 1000,
            scale: 1.2,
            primary: false,
            known: true,
            active: true,
            to: String::new(),
            side: None,
            offset: 0,
        }
    }

    #[test]
    fn the_summary_names_each_monitor() {
        let two = [monitor("eDP-1", "Unknown"), monitor("DP-3", "LG HDR WQHD")];
        assert_eq!(
            summary(&two),
            "Built-in display (1920×1200)  ·  LG HDR WQHD (1920×1200)"
        );
        assert_eq!(summary(&two[..1]), "Only one display is connected");
    }

    #[test]
    fn it_starts_on_the_mode_not_in_use() {
        let two = [monitor("a", ""), monitor("b", "")];
        let hud = Hud::new(Skin::default_skin(), &two, Mode::Extend, Rc::default());
        assert_eq!(CHOICES[hud.current], Mode::Mirror);
    }

    fn waiting() -> [Monitor; 2] {
        let mut off = monitor("b", "");
        off.active = false;
        [monitor("a", ""), off]
    }

    #[test]
    fn a_monitor_waiting_starts_on_extend_and_is_named_off() {
        let two = waiting();
        let hud = Hud::new(Skin::default_skin(), &two, Mode::Off, Rc::default());
        assert_eq!(CHOICES[hud.current], Mode::Extend);
        assert_eq!(summary(&two), "a (1920×1200)  ·  b (1920×1200, off)");
        assert_eq!(label(Mode::Off, true), "Do nothing");
        assert_eq!(label(Mode::Off, false), "Primary only");
    }

    #[test]
    fn doing_nothing_closes_without_choosing() {
        let two = waiting();
        let mut hud = Hud::new(Skin::default_skin(), &two, Mode::Off, Rc::default());
        hud.choose(Mode::Off);
        assert_eq!((hud.chosen.get(), hud.done), (None, true));
    }

    #[test]
    fn primary_only_is_chosen_when_nothing_waits() {
        let two = [monitor("a", ""), monitor("b", "")];
        let mut hud = Hud::new(Skin::default_skin(), &two, Mode::Extend, Rc::default());
        hud.choose(Mode::Off);
        assert_eq!((hud.chosen.get(), hud.done), (Some(Mode::Off), true));
    }

    #[test]
    fn one_monitor_chooses_nothing() {
        let one = [monitor("a", "")];
        let mut hud = Hud::new(Skin::default_skin(), &one, Mode::Extend, Rc::default());
        hud.choose(Mode::Mirror);
        assert_eq!((hud.chosen.get(), hud.done), (None, false));
    }

    #[test]
    fn choosing_finishes() {
        let two = [monitor("a", ""), monitor("b", "")];
        let mut hud = Hud::new(Skin::default_skin(), &two, Mode::Extend, Rc::default());
        hud.choose(Mode::Mirror);
        assert_eq!((hud.chosen.get(), hud.done), (Some(Mode::Mirror), true));
    }
}
