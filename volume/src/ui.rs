//! The volume menu: a mute button, a slider and the level, at the bar's
//! right end. As a menu it stays until Escape, a click elsewhere or the
//! bar's button again; as a flash, after a volume or brightness key, it
//! fades in, holds and fades out on its own, showing what the key changed
//! (ADR-0033). Keys pressed again keep it up, without fading again.

use egui::{Align, Layout, Sense, Slider, StrokeKind, Ui, UiBuilder, pos2, vec2};
use patina::skin::Skin;
use patina_shell::Shell;

use patina_system::audio::Volume;
use patina_system::mpris::{Command, Player, Status};

/// Fade in, hold, fade out, in seconds, for a flash.
const FADE_IN: f64 = 0.12;
const HOLD: f64 = 1.5;
const FADE_OUT: f64 = 0.3;
const MARGIN: f32 = 12.0;
/// The slider's part of the menu, and each player's card under it (ADR-0117).
const TOP: f32 = 56.0;
const CARD: f32 = 48.0;
const CARD_GAP: f32 = 6.0;
/// Players shown before the list scrolls.
const MAX_CARDS: usize = 4;
/// A card's control buttons.
const BUTTON: f32 = 28.0;

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

/// How tall the menu is with `players` of them listed: the slider, and a card each.
pub fn height(players: usize) -> u32 {
    let shown = players.min(MAX_CARDS) as f32;
    if shown == 0.0 {
        return TOP as u32;
    }
    (TOP - 2.0 + shown * CARD + (shown - 1.0) * CARD_GAP + MARGIN).round() as u32
}

pub struct VolumeMenu {
    pub volume: Option<Volume>,
    /// The players with something to control, playing first; listed in the menu, not the flash.
    pub players: Vec<Player>,
    /// What the cards ask of a player, to the thread that sends it.
    commands: std::sync::mpsc::Sender<(String, Command)>,
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
        commands: std::sync::mpsc::Sender<(String, Command)>,
    ) -> Self {
        Self {
            volume: None,
            players: Vec::new(),
            commands,
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

    /// A card for `player`: what it's playing, and its controls.
    fn card(&mut self, ui: &mut Ui, index: usize) {
        let v = ui.visuals().clone();
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), CARD), Sense::hover());
        ui.painter().rect_filled(
            rect,
            v.widgets.noninteractive.corner_radius,
            v.faint_bg_color,
        );
        let player = self.players[index].clone();
        // The buttons at the right, the text in what's left.
        let mut right = rect.right() - 6.0;
        let mut clicked = None;
        let buttons = [
            (Command::Next, player.can_next),
            (Command::PlayPause, true),
            (Command::Previous, player.can_previous),
        ];
        for (command, enabled) in buttons {
            let at = egui::Rect::from_center_size(
                pos2(right - BUTTON / 2.0, rect.center().y),
                vec2(BUTTON, BUTTON),
            );
            right -= BUTTON;
            if !enabled {
                continue;
            }
            let response = ui.interact(
                at,
                ui.id().with((index, command_name(command))),
                Sense::click(),
            );
            if response.hovered() {
                ui.painter().rect_filled(
                    at,
                    v.widgets.hovered.corner_radius,
                    v.widgets.hovered.weak_bg_fill,
                );
            }
            let colour = if response.hovered() {
                v.strong_text_color()
            } else {
                v.text_color()
            };
            let glyph = at.shrink(7.0);
            match command {
                Command::Previous => patina_system::icons::skip(ui.painter(), glyph, false, colour),
                Command::Next => patina_system::icons::skip(ui.painter(), glyph, true, colour),
                Command::PlayPause if player.status == Status::Playing => {
                    patina_system::icons::pause(ui.painter(), glyph, colour)
                }
                Command::PlayPause => patina_system::icons::play(ui.painter(), glyph, colour),
            }
            if response.clicked() {
                clicked = Some(command);
            }
        }
        let text = egui::Rect::from_min_max(
            pos2(rect.left() + 10.0, rect.top()),
            pos2(right - 4.0, rect.bottom()),
        );
        let by = if player.artist.is_empty() {
            player.identity.clone()
        } else {
            format!("{} · {}", player.artist, player.identity)
        };
        // A child, not a scope: a scope would move the list's cursor back up to the end of the
        // text, so each card would start inside the one above and the gap would be lost.
        let mut label = ui.new_child(
            UiBuilder::new()
                .max_rect(text)
                .layout(Layout::top_down(Align::Min)),
        );
        label.add_space(7.0);
        label.add(egui::Label::new(egui::RichText::new(&player.title).strong()).truncate());
        label.add(egui::Label::new(egui::RichText::new(by).weak().small()).truncate());
        if let Some(command) = clicked {
            let _ = self.commands.send((player.bus_name.clone(), command));
            // Show it at once; the next read of the players confirms it.
            if command == Command::PlayPause
                && let Some(p) = self.players.get_mut(index)
            {
                p.status = if p.status == Status::Playing {
                    Status::Paused
                } else {
                    Status::Playing
                };
            }
        }
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
        // The slider's part; under it, in the menu, a card for each player (ADR-0117).
        let top = egui::Rect::from_min_size(rect.min, vec2(rect.width(), TOP.min(rect.height())));
        if let Some(missing) = missing {
            ui.painter().text(
                top.center(),
                egui::Align2::CENTER_CENTER,
                missing,
                egui::TextStyle::Body.resolve(ui.style()),
                v.weak_text_color(),
            );
        } else {
            let mut row = ui.new_child(
                UiBuilder::new()
                    .max_rect(top.shrink(MARGIN))
                    .layout(Layout::left_to_right(Align::Center)),
            );
            self.level_row(&mut row, level, muted);
        }
        if self.mode != Mode::Menu || self.players.is_empty() {
            return;
        }
        let cards = egui::Rect::from_min_max(
            pos2(rect.left() + MARGIN, top.bottom() - 2.0),
            pos2(rect.right() - MARGIN, rect.bottom() - MARGIN),
        );
        let mut list = ui.new_child(
            UiBuilder::new()
                .max_rect(cards)
                .layout(Layout::top_down(Align::Min)),
        );
        list.spacing_mut().item_spacing.y = CARD_GAP;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(&mut list, |ui| {
                for index in 0..self.players.len() {
                    self.card(ui, index);
                }
            });
    }

    fn finished(&self) -> bool {
        self.done
    }
}

fn command_name(command: Command) -> &'static str {
    match command {
        Command::PlayPause => "play",
        Command::Next => "next",
        Command::Previous => "previous",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_menu_grows_with_its_players_up_to_a_few() {
        assert_eq!(height(0), 56, "no players: just the slider, as before");
        assert!(height(1) > height(0));
        assert!(height(2) > height(1));
        assert_eq!(height(MAX_CARDS + 3), height(MAX_CARDS), "then it scrolls");
    }

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
