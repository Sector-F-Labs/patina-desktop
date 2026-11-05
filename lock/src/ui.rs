//! The lock screen: the time and date on the skin's background on every
//! output, and on the one with the keyboard, who's locked and a box for
//! their password.
//!
//! The password isn't kept in a TextEdit, whose undo history would hold
//! copies of it: keys are read straight from egui's input, only dots are
//! shown, and it's wiped once checked or cleared.

use std::collections::HashMap;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use egui::{Align2, CornerRadius, FontId, RichText, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use patina::skin::Skin;
use patina_shell::Shell;

const FIELD: egui::Vec2 = vec2(320.0, 46.0);
/// The heights, in points, of the clock and date, and of the picture, name,
/// box and message under them, to centre them by.
const CLOCK: f32 = 140.0;
const LOGIN: f32 = 210.0;
/// How long the box shakes after a wrong password.
const SHAKE: f32 = 0.45;
/// Longer than any password; what's typed past it is dropped.
const MAX_PASSWORD: usize = 1024;

/// Where checking the password is.
#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Typing,
    Checking,
    Wrong,
    /// PAM couldn't check it at all.
    Problem(String),
}

pub struct LockScreen {
    skin: Skin,
    /// The name shown: the full name, or the login.
    name: String,
    password: String,
    status: Status,
    /// When the last password was refused, for the shake.
    wrong_at: Option<Instant>,
    unlocked: bool,
    check: Sender<String>,
    /// The user's picture, if they have one, and its texture on each output.
    face: Option<egui::ColorImage>,
    faces: HashMap<String, egui::TextureHandle>,
}

impl LockScreen {
    pub fn new(skin: Skin, name: String, check: Sender<String>) -> Self {
        Self {
            skin,
            name,
            password: String::with_capacity(MAX_PASSWORD),
            status: Status::Typing,
            wrong_at: None,
            unlocked: false,
            check,
            face: patina_system::avatar::load(),
            faces: HashMap::new(),
        }
    }

    /// PAM's answer for the password last sent.
    pub fn checked(&mut self, result: Result<bool, String>) {
        self.status = match result {
            Ok(true) => {
                self.unlocked = true;
                Status::Typing
            }
            Ok(false) => {
                self.wrong_at = Some(Instant::now());
                Status::Wrong
            }
            Err(e) => Status::Problem(e),
        };
    }

    /// Sends the password to be checked, if there is one.
    fn submit(&mut self) {
        if self.password.is_empty() || self.status == Status::Checking {
            return;
        }
        let password = std::mem::replace(&mut self.password, String::with_capacity(MAX_PASSWORD));
        if self.check.send(password).is_ok() {
            self.status = Status::Checking;
        }
    }

    /// Keys typed into the password box.
    fn typed(&mut self, ui: &Ui) {
        if self.status == Status::Checking {
            return;
        }
        let events = ui.input(|i| i.events.clone());
        for event in events {
            match event {
                egui::Event::Text(text) => {
                    if self.password.len() + text.len() <= MAX_PASSWORD {
                        self.password.push_str(&text);
                    }
                    self.status = Status::Typing;
                }
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => match key {
                    egui::Key::Enter => self.submit(),
                    egui::Key::Backspace if modifiers.ctrl => wipe(&mut self.password),
                    egui::Key::Backspace => {
                        self.password.pop();
                    }
                    egui::Key::Escape => wipe(&mut self.password),
                    egui::Key::U if modifiers.ctrl => wipe(&mut self.password),
                    _ => {}
                },
                _ => {}
            }
        }
    }

    /// The user's picture in a circle, or a disc with the name's first
    /// letter, in the skin's accent.
    fn avatar(&mut self, ui: &mut Ui, output: &str) {
        let side = 84.0;
        let (rect, _) = ui.allocate_exact_size(vec2(side, side), Sense::hover());
        if let Some(face) = &self.face {
            let texture = self.faces.entry(output.to_string()).or_insert_with(|| {
                ui.ctx()
                    .load_texture("face", face.clone(), egui::TextureOptions::LINEAR)
            });
            egui::Image::from_texture(&*texture)
                .corner_radius(side / 2.0)
                .paint_at(ui, rect);
            ui.painter().circle_stroke(
                rect.center(),
                side / 2.0,
                Stroke::new(2.0, ui.visuals().selection.bg_fill),
            );
            return;
        }
        let v = ui.visuals();
        ui.painter()
            .circle_filled(rect.center(), side / 2.0, v.selection.bg_fill);
        let initial = self
            .name
            .chars()
            .next()
            .map_or(String::new(), |c| c.to_uppercase().to_string());
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            initial,
            FontId::proportional(38.0),
            v.selection.stroke.color,
        );
    }

    /// The pill the password goes in: a ring while it has the keys, a dot
    /// for each character and a caret after them, a button that sends it,
    /// a spinner while PAM checks, and a shake when it's refused.
    fn password_box(&mut self, ui: &mut Ui) {
        let visuals = ui.visuals().clone();
        let time = ui.input(|i| i.time);
        let (rect, _) = ui.allocate_exact_size(FIELD, Sense::hover());
        let shake = self
            .wrong_at
            .map(|t| t.elapsed().as_secs_f32())
            .filter(|s| *s < SHAKE)
            .map_or(0.0, |s| (s * 45.0).sin() * 10.0 * (1.0 - s / SHAKE));
        let rect = rect.translate(vec2(shake, 0.0));
        let checking = self.status == Status::Checking;
        let radius = CornerRadius::same((FIELD.y / 2.0) as u8);
        let painter = ui.painter().clone();
        painter.rect_filled(rect, radius, visuals.extreme_bg_color);
        let ring = if self.status == Status::Wrong {
            Stroke::new(2.0, visuals.error_fg_color)
        } else {
            Stroke::new(2.0, visuals.selection.bg_fill)
        };
        painter.rect_stroke(rect, radius, ring, StrokeKind::Inside);

        // The button on the right.
        let button = egui::Rect::from_center_size(
            pos2(rect.right() - FIELD.y / 2.0, rect.center().y),
            vec2(FIELD.y - 12.0, FIELD.y - 12.0),
        );
        let ready = !self.password.is_empty() && !checking;
        let response = ui.interact(button, ui.id().with("unlock"), Sense::click());
        if ready && response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let (fill, ink) = if ready {
            let fill = if response.hovered() {
                visuals.selection.bg_fill.gamma_multiply(0.85)
            } else {
                visuals.selection.bg_fill
            };
            (fill, visuals.selection.stroke.color)
        } else {
            (
                visuals.widgets.inactive.weak_bg_fill,
                visuals.weak_text_color(),
            )
        };
        painter.circle_filled(button.center(), button.width() / 2.0, fill);
        let c = button.center();
        if checking {
            // A dot going round.
            let angle = time as f32 * 6.0;
            for k in 0..3 {
                let a = angle - k as f32 * 0.5;
                painter.circle_filled(
                    c + vec2(a.cos(), a.sin()) * 8.0,
                    2.6 - k as f32 * 0.7,
                    ink.gamma_multiply(1.0 - k as f32 * 0.3),
                );
            }
        } else {
            let line = Stroke::new(2.0, ink);
            painter.line_segment([c - vec2(6.0, 0.0), c + vec2(6.0, 0.0)], line);
            painter.line_segment([c + vec2(6.0, 0.0), c + vec2(1.0, -5.0)], line);
            painter.line_segment([c + vec2(6.0, 0.0), c + vec2(1.0, 5.0)], line);
        }
        if ready && response.clicked() {
            self.submit();
        }

        // Dots from the left, the newest last, then the caret.
        let left = rect.left() + 22.0;
        let room = button.left() - 14.0 - left;
        let step = 15.0;
        let fit = ((room / step) as usize).max(1);
        let count = self.password.chars().count();
        let shown = count.min(fit);
        let dim = if checking { 0.45 } else { 1.0 };
        for i in 0..shown {
            painter.circle_filled(
                pos2(left + 4.0 + i as f32 * step, rect.center().y),
                4.5,
                visuals.text_color().gamma_multiply(dim),
            );
        }
        if count == 0 {
            painter.text(
                pos2(left + 6.0, rect.center().y),
                Align2::LEFT_CENTER,
                "Password",
                FontId::proportional(16.0),
                visuals.weak_text_color(),
            );
        }
        let blink = (time * 2.0) as i64 % 2 == 0;
        if !checking && blink {
            let x = left - 2.0 + shown as f32 * step;
            let x = x.min(left - 2.0 + fit as f32 * step);
            painter.line_segment(
                [
                    pos2(x, rect.center().y - 10.0),
                    pos2(x, rect.center().y + 10.0),
                ],
                Stroke::new(1.5, visuals.text_color()),
            );
        }
    }
}

impl Shell for LockScreen {
    fn setup(&mut self, ctx: &egui::Context) {
        self.skin.apply(ctx);
        if self.skin.follows_desktop() {
            patina::appearance::watch(ctx);
        }
    }

    fn ui(&mut self, ui: &mut Ui, output: &str) {
        let focused = ui.input(|i| i.focused);
        if focused {
            self.typed(ui);
        }
        let rect = ui.max_rect();
        ui.painter().rect_filled(rect, 0.0, ui.visuals().panel_fill);

        let now = chrono::Local::now();
        ui.vertical_centered(|ui| {
            // The clock goes near the top where there's a login below it, and
            // in the middle where there isn't.
            ui.add_space(if focused {
                rect.height() * 0.14
            } else {
                (rect.height() - CLOCK).max(0.0) / 2.0
            });
            ui.label(
                RichText::new(now.format("%H:%M").to_string())
                    .size(96.0)
                    .strong(),
            );
            ui.label(RichText::new(now.format("%A %-d %B").to_string()).size(22.0));
            if !focused {
                return;
            }
            // The picture, name and box are centred on the screen as one.
            let top = rect.center().y - LOGIN / 2.0;
            ui.add_space((top - ui.cursor().top()).max(32.0));
            self.avatar(ui, output);
            ui.add_space(10.0);
            ui.label(RichText::new(&self.name).size(20.0));
            ui.add_space(14.0);
            self.password_box(ui);
            ui.add_space(10.0);
            let visuals = ui.visuals();
            let (text, colour) = match &self.status {
                Status::Typing => ("", visuals.weak_text_color()),
                Status::Checking => ("Checking…", visuals.weak_text_color()),
                Status::Wrong => ("Wrong password", visuals.error_fg_color),
                Status::Problem(e) => (e.as_str(), visuals.error_fg_color),
            };
            ui.label(RichText::new(text).color(colour));
        });

        // The minute changes on its own; the caret, spinner and shake need
        // frames of their own while they show.
        let into_minute = Duration::from_millis(now.timestamp_subsec_millis().into())
            + Duration::from_secs(now.timestamp().rem_euclid(60) as u64);
        let mut next = Duration::from_secs(60).saturating_sub(into_minute);
        if focused {
            let shaking = self
                .wrong_at
                .is_some_and(|t| t.elapsed().as_secs_f32() < SHAKE);
            next = next.min(if self.status == Status::Checking || shaking {
                Duration::from_millis(16)
            } else {
                Duration::from_millis(500)
            });
        }
        ui.ctx().request_repaint_after(next);
    }

    fn finished(&self) -> bool {
        self.unlocked
    }
}

/// Clears a password, zeroing its bytes first.
pub fn wipe(password: &mut String) {
    let mut bytes = std::mem::take(password).into_bytes();
    bytes.fill(0);
    std::hint::black_box(&bytes);
    password.reserve(MAX_PASSWORD);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wiping_empties_and_keeps_room() {
        let mut password = String::from("hunter2");
        wipe(&mut password);
        assert!(password.is_empty());
        assert!(password.capacity() >= MAX_PASSWORD);
    }
}
