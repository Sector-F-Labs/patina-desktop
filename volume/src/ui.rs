//! The volume menu: a mute button, a slider and the level, at the bar's
//! right end. As a menu it stays until Escape, a click elsewhere or the
//! bar's button again; as a flash, after a volume or brightness key, it
//! fades in, holds and fades out on its own, showing what the key changed
//! (ADR-0033). Keys pressed again keep it up, without fading again.

use egui::{Align, Layout, Sense, Slider, StrokeKind, Ui, UiBuilder, vec2};
use patina::skin::Skin;
use patina_shell::Shell;

use patina_system::audio::Volume;

/// Fade in, hold, fade out, in seconds, for a flash.
const FADE_IN: f64 = 0.12;
const HOLD: f64 = 1.5;
const FADE_OUT: f64 = 0.3;
const MARGIN: f32 = 12.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mode {
    /// Opened from the bar: takes the keyboard, stays until closed.
    Menu,
    /// Shown by a key: fades out on its own.
    Flash,
}

/// What it shows: the menu is always the volume; a flash shows what its
/// key changed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Showing {
    Volume,
    Brightness,
}

/// What the user asked of the audio or the backlight.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Change {
    Level(f32),
    Muted(bool),
    Brightness(f32),
}

pub struct VolumeMenu {
    pub volume: Option<Volume>,
    /// The backlight, 0 to 1; `None` without one.
    pub brightness: Option<f32>,
    mode: Mode,
    showing: Showing,
    /// When the menu was shown, in egui's time; set on its first frame.
    shown_at: Option<f64>,
    /// A key again: hold from now, on the next frame.
    again: bool,
    had_focus: bool,
    done: bool,
    skin: Skin,
    /// Changes, to the thread that applies them.
    changes: std::sync::mpsc::Sender<Change>,
}

impl VolumeMenu {
    pub fn new(
        mode: Mode,
        showing: Showing,
        skin: Skin,
        changes: std::sync::mpsc::Sender<Change>,
    ) -> Self {
        Self {
            volume: None,
            brightness: None,
            mode,
            showing,
            shown_at: None,
            again: false,
            had_focus: false,
            done: false,
            skin,
            changes,
        }
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// A key again: show what it changed, and hold from now. As a menu,
    /// just carry on.
    pub fn flash(&mut self, showing: Showing) {
        if self.mode == Mode::Flash {
            self.showing = showing;
            self.again = true;
        }
    }

    /// The bar's button while flashing: become the menu.
    pub fn open(&mut self) {
        self.mode = Mode::Menu;
        self.showing = Showing::Volume;
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

    /// Where a flash `t` seconds along goes when a key comes again: still
    /// fading in, it carries on; shown or fading out, it's fully shown and
    /// holds from the start. Never back to the start of the fade, which
    /// made every press blink.
    pub fn held_again(t: f64) -> f64 {
        t.min(FADE_IN)
    }

    /// A speaker or a sun, the slider and the percentage, for `level`.
    fn level_row(&mut self, ui: &mut Ui, level: f32, muted: bool) {
        let v = ui.visuals().clone();
        let side = ui.available_height().min(28.0);
        let (icon, response) = ui.allocate_exact_size(vec2(side, side), Sense::click());
        let volume = self.showing == Showing::Volume;
        if volume && response.hovered() {
            ui.painter().rect_filled(
                icon,
                v.widgets.hovered.corner_radius,
                v.widgets.hovered.weak_bg_fill,
            );
        }
        if volume {
            patina_system::icons::speaker(
                ui.painter(),
                icon.shrink(3.0),
                level,
                muted,
                v.text_color(),
            );
            if response.clicked() {
                let _ = self.changes.send(Change::Muted(!muted));
                if let Some(volume) = &mut self.volume {
                    volume.muted = !muted;
                }
            }
        } else {
            patina_system::icons::sun(ui.painter(), icon.shrink(3.0), level, v.text_color());
        }

        let percent = format!("{:>3}%", (level * 100.0).round() as i32);
        let label_width = 44.0;
        let mut slid = level.clamp(0.0, 1.0);
        ui.spacing_mut().slider_width = (ui.available_width() - label_width - 8.0).max(40.0);
        let slider = ui.add(
            Slider::new(&mut slid, 0.0..=1.0)
                .show_value(false)
                .trailing_fill(true),
        );
        if slider.changed() {
            if volume {
                let _ = self.changes.send(Change::Level(slid));
                if let Some(volume) = &mut self.volume {
                    volume.level = slid;
                }
            } else {
                let _ = self.changes.send(Change::Brightness(slid));
                self.brightness = Some(slid);
            }
            // Moving the slider keeps a flash up, as a key would.
            self.flash(self.showing);
        }
        ui.add_sized(vec2(label_width, side), egui::Label::new(percent));
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
        if std::mem::take(&mut self.again) {
            self.shown_at = Some(now - Self::held_again(now - shown_at));
        }
        let t = now - self.shown_at.unwrap_or(now);

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
        let (level, muted, missing) = match self.showing {
            Showing::Volume => match self.volume {
                Some(volume) => (volume.level, volume.muted, None),
                None => (0.0, false, Some("No sound output")),
            },
            Showing::Brightness => match self.brightness {
                Some(level) => (level, false, None),
                None => (0.0, false, Some("No backlight")),
            },
        };
        if let Some(missing) = missing {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                missing,
                egui::TextStyle::Body.resolve(ui.style()),
                v.weak_text_color(),
            );
            return;
        }
        let mut ui = ui.new_child(
            UiBuilder::new()
                .max_rect(rect.shrink(MARGIN))
                .layout(Layout::left_to_right(Align::Center)),
        );
        self.level_row(&mut ui, level, muted);
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

    /// Keys again used to send it back to t = 0, transparent: a blink.
    #[test]
    fn a_key_again_stays_shown_and_holds_from_now() {
        let held = |t| VolumeMenu::opacity(Mode::Flash, VolumeMenu::held_again(t));
        assert_eq!(held(1.0), 1.0, "shown: stays shown");
        assert_eq!(
            held(FADE_IN + HOLD + FADE_OUT * 0.9),
            1.0,
            "fading out: back up"
        );
        assert_eq!(
            held(FADE_IN / 2.0),
            VolumeMenu::opacity(Mode::Flash, FADE_IN / 2.0)
        );
        let t = VolumeMenu::held_again(FADE_IN + HOLD);
        assert!(
            !VolumeMenu::faded(Mode::Flash, t + HOLD * 0.9),
            "a full hold again"
        );
    }
}
