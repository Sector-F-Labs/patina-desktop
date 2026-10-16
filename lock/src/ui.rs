//! The lock screen: the time and date on the skin's background on every
//! output, and on the one with the keyboard, who's locked and a box for
//! their password.
//!
//! The password isn't kept in a TextEdit, whose undo history would hold
//! copies of it: keys are read straight from egui's input, only dots are
//! shown, and it's wiped once checked or cleared.

use std::sync::mpsc::Sender;
use std::time::Duration;

use egui::{Align2, FontId, Rect, RichText, Sense, StrokeKind, Ui, vec2};
use patina::skin::Skin;
use patina_shell::Shell;

const FIELD: egui::Vec2 = vec2(300.0, 40.0);
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
    unlocked: bool,
    check: Sender<String>,
}

impl LockScreen {
    pub fn new(skin: Skin, name: String, check: Sender<String>) -> Self {
        Self {
            skin,
            name,
            password: String::with_capacity(MAX_PASSWORD),
            status: Status::Typing,
            unlocked: false,
            check,
        }
    }

    /// PAM's answer for the password last sent.
    pub fn checked(&mut self, result: Result<bool, String>) {
        self.status = match result {
            Ok(true) => {
                self.unlocked = true;
                Status::Typing
            }
            Ok(false) => Status::Wrong,
            Err(e) => Status::Problem(e),
        };
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
                    egui::Key::Enter if !self.password.is_empty() => {
                        let password = std::mem::replace(
                            &mut self.password,
                            String::with_capacity(MAX_PASSWORD),
                        );
                        if self.check.send(password).is_ok() {
                            self.status = Status::Checking;
                        }
                    }
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

    fn password_box(&self, ui: &mut Ui) {
        let visuals = ui.visuals().clone();
        let (rect, _) = ui.allocate_exact_size(FIELD, Sense::hover());
        let painter = ui.painter();
        let radius = visuals.widgets.inactive.corner_radius;
        painter.rect_filled(rect, radius, visuals.extreme_bg_color);
        let stroke = if self.status == Status::Wrong {
            egui::Stroke::new(1.5, visuals.error_fg_color)
        } else {
            visuals.selection.stroke
        };
        painter.rect_stroke(rect, radius, stroke, StrokeKind::Inside);
        let inner = Rect::from_center_size(rect.center(), rect.size() - vec2(24.0, 0.0));
        if self.password.is_empty() {
            painter.text(
                inner.left_center(),
                Align2::LEFT_CENTER,
                "Password",
                FontId::proportional(16.0),
                visuals.weak_text_color(),
            );
        } else {
            // One dot per character, as many as fit.
            let dots = self.password.chars().count().min(24);
            let step = 14.0;
            let start = rect.center().x - (dots as f32 - 1.0) * step / 2.0;
            for i in 0..dots {
                painter.circle_filled(
                    egui::pos2(start + i as f32 * step, rect.center().y),
                    4.0,
                    visuals.text_color(),
                );
            }
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

    fn ui(&mut self, ui: &mut Ui, _: &str) {
        let focused = ui.input(|i| i.focused);
        if focused {
            self.typed(ui);
        }
        let rect = ui.max_rect();
        ui.painter().rect_filled(rect, 0.0, ui.visuals().panel_fill);

        let now = chrono::Local::now();
        ui.vertical_centered(|ui| {
            ui.add_space(rect.height() * 0.28);
            ui.label(
                RichText::new(now.format("%H:%M").to_string())
                    .size(96.0)
                    .strong(),
            );
            ui.label(RichText::new(now.format("%A %-d %B").to_string()).size(22.0));
            if !focused {
                return;
            }
            ui.add_space(56.0);
            ui.label(RichText::new(&self.name).size(18.0));
            ui.add_space(12.0);
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

        // The minute changes on its own.
        let into_minute = Duration::from_millis(now.timestamp_subsec_millis().into())
            + Duration::from_secs(now.timestamp().rem_euclid(60) as u64);
        ui.ctx()
            .request_repaint_after(Duration::from_secs(60).saturating_sub(into_minute));
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
