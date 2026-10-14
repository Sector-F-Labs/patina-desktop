//! The volume menu: a mute button, a slider and the level, at the bar's
//! right end. As a menu it stays until Escape, a click elsewhere or the
//! bar's button again; as a flash, after a volume key, it fades in, holds
//! and fades out on its own.

use egui::{Align, Layout, Sense, Slider, StrokeKind, Ui, UiBuilder, vec2};
use patina::skin::Skin;
use patina_shell::Shell;

use patina_shell::audio::Volume;

/// Fade in, hold, fade out, in seconds, for a flash.
const FADE_IN: f64 = 0.12;
const HOLD: f64 = 1.5;
const FADE_OUT: f64 = 0.3;
const MARGIN: f32 = 12.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mode {
    /// Opened from the bar: takes the keyboard, stays until closed.
    Menu,
    /// Shown by a volume key: fades out on its own.
    Flash,
}

/// What the user asked of the audio.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Change {
    Level(f32),
    Muted(bool),
}

pub struct VolumeMenu {
    pub volume: Option<Volume>,
    mode: Mode,
    /// When the menu was (re)shown, in egui's time; set on the next frame
    /// after a show.
    shown_at: Option<f64>,
    had_focus: bool,
    done: bool,
    skin: Skin,
    /// Changes, to the audio thread.
    changes: std::sync::mpsc::Sender<Change>,
}

impl VolumeMenu {
    pub fn new(mode: Mode, skin: Skin, changes: std::sync::mpsc::Sender<Change>) -> Self {
        Self {
            volume: None,
            mode,
            shown_at: None,
            had_focus: false,
            done: false,
            skin,
            changes,
        }
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// A volume key again: show it from the start of the hold, or, as a
    /// menu, just carry on.
    pub fn flash(&mut self) {
        if self.mode == Mode::Flash {
            self.shown_at = None;
        }
    }

    /// The bar's button while flashing: become the menu.
    pub fn open(&mut self) {
        self.mode = Mode::Menu;
    }

    pub fn close(&mut self) {
        self.done = true;
    }

    /// How opaque the menu is `t` seconds after it was shown.
    pub fn opacity(mode: Mode, t: f64) -> f32 {
        let fade_in = (t / FADE_IN).clamp(0.0, 1.0);
        let fade_out = match mode {
            Mode::Menu => 1.0,
            Mode::Flash => (1.0 - (t - FADE_IN - HOLD) / FADE_OUT).clamp(0.0, 1.0),
        };
        fade_in.min(fade_out) as f32
    }

    /// Whether a flash shown `t` seconds ago has faded out.
    pub fn faded(mode: Mode, t: f64) -> bool {
        mode == Mode::Flash && t >= FADE_IN + HOLD + FADE_OUT
    }
}

impl Shell for VolumeMenu {
    fn setup(&mut self, ctx: &egui::Context) {
        self.skin.apply(ctx);
        if self.skin.follows_desktop() {
            patina::appearance::watch(ctx);
        }
    }

    fn ui(&mut self, ui: &mut Ui, _: &str) {
        let now = ui.input(|i| i.time);
        let shown_at = *self.shown_at.get_or_insert(now);
        let t = now - shown_at;

        if self.mode == Mode::Menu {
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
        }
        if Self::faded(self.mode, t) {
            self.done = true;
            return;
        }
        let opacity = Self::opacity(self.mode, t);
        if opacity < 1.0 || self.mode == Mode::Flash {
            ui.ctx().request_repaint();
        }
        ui.set_opacity(opacity);

        let rect = ui.max_rect();
        let v = ui.visuals().clone();
        ui.painter().rect(
            rect,
            v.menu_corner_radius,
            v.panel_fill,
            v.window_stroke,
            StrokeKind::Inside,
        );
        let Some(volume) = self.volume else {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "No sound output",
                egui::TextStyle::Body.resolve(ui.style()),
                v.weak_text_color(),
            );
            return;
        };

        let mut ui = ui.new_child(
            UiBuilder::new()
                .max_rect(rect.shrink(MARGIN))
                .layout(Layout::left_to_right(Align::Center)),
        );
        let side = ui.available_height().min(28.0);
        let (icon, response) = ui.allocate_exact_size(vec2(side, side), Sense::click());
        if response.hovered() {
            ui.painter().rect_filled(
                icon,
                v.widgets.hovered.corner_radius,
                v.widgets.hovered.weak_bg_fill,
            );
        }
        patina_shell::icons::speaker(
            ui.painter(),
            icon.shrink(3.0),
            volume.level,
            volume.muted,
            v.text_color(),
        );
        if response.clicked() {
            let _ = self.changes.send(Change::Muted(!volume.muted));
            self.volume = Some(Volume {
                muted: !volume.muted,
                ..volume
            });
        }

        let percent = format!("{:>3}%", (volume.level * 100.0).round() as i32);
        let label_width = 44.0;
        let mut level = volume.level.clamp(0.0, 1.0);
        ui.spacing_mut().slider_width = (ui.available_width() - label_width - 8.0).max(40.0);
        let slider = ui.add(
            Slider::new(&mut level, 0.0..=1.0)
                .show_value(false)
                .trailing_fill(true),
        );
        if slider.changed() {
            let _ = self.changes.send(Change::Level(level));
            self.volume = Some(Volume { level, ..volume });
            // Moving the slider keeps a flash up, as a key would.
            self.flash();
        }
        ui.add_sized(vec2(label_width, side), egui::Label::new(percent));
    }

    fn finished(&self) -> bool {
        self.done
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flash_fades_in_holds_and_fades_out() {
        assert_eq!(VolumeMenu::opacity(Mode::Flash, 0.0), 0.0);
        assert!((VolumeMenu::opacity(Mode::Flash, FADE_IN / 2.0) - 0.5).abs() < 0.01);
        assert_eq!(VolumeMenu::opacity(Mode::Flash, 1.0), 1.0);
        let fading = FADE_IN + HOLD + FADE_OUT / 2.0;
        assert!((VolumeMenu::opacity(Mode::Flash, fading) - 0.5).abs() < 0.01);
        assert!(!VolumeMenu::faded(Mode::Flash, 1.0));
        assert!(VolumeMenu::faded(Mode::Flash, FADE_IN + HOLD + FADE_OUT));
    }

    #[test]
    fn a_menu_fades_in_and_stays() {
        assert_eq!(VolumeMenu::opacity(Mode::Menu, 0.0), 0.0);
        assert_eq!(VolumeMenu::opacity(Mode::Menu, 60.0), 1.0);
        assert!(!VolumeMenu::faded(Mode::Menu, 60.0));
    }
}
