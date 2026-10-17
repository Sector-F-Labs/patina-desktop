//! The power menu: Lock, Log Out…, Restart… and Shut Down…. The three that
//! end the session ask first, in the menu's place, since every app closes;
//! what this machine can't do is shown greyed out.

use std::sync::mpsc::Sender;

use egui::{Align, Layout, RichText, Sense, StrokeKind, Ui, UiBuilder, vec2};
use patina::skin::Skin;
use patina_shell::Shell;
use patina_system::session::Abilities;

const MARGIN: f32 = 8.0;
const ROW: f32 = 40.0;

/// What the menu can be asked to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Lock,
    LogOut,
    Restart,
    ShutDown,
}

impl Action {
    const ALL: [Action; 4] = [
        Action::Lock,
        Action::LogOut,
        Action::Restart,
        Action::ShutDown,
    ];

    fn label(self) -> &'static str {
        match self {
            Action::Lock => "Lock",
            Action::LogOut => "Log Out…",
            Action::Restart => "Restart…",
            Action::ShutDown => "Shut Down…",
        }
    }

    /// The question, and the button that answers it.
    fn question(self) -> (&'static str, &'static str) {
        match self {
            Action::Lock => ("", "Lock"),
            Action::LogOut => ("Log out?", "Log Out"),
            Action::Restart => ("Restart the computer?", "Restart"),
            Action::ShutDown => ("Shut down the computer?", "Shut Down"),
        }
    }

    /// Whether it's done at once, without asking.
    fn immediate(self) -> bool {
        self == Action::Lock
    }

    fn possible(self, a: &Abilities) -> bool {
        match self {
            // patina-lock can be run directly where the compositor can't.
            Action::Lock => true,
            Action::LogOut => a.session,
            Action::Restart => a.restart,
            Action::ShutDown => a.shut_down,
        }
    }

    /// Why it's greyed out.
    fn unavailable(self) -> &'static str {
        match self {
            Action::LogOut => "Only in a Patina session",
            _ => "Not allowed here, or no login manager",
        }
    }
}

pub struct PowerMenu {
    /// What can be done here; `None` until asked.
    pub abilities: Option<Abilities>,
    /// The action waiting for an answer.
    asking: Option<Action>,
    /// What went wrong with the last one.
    problem: Option<String>,
    /// An action is being carried out.
    busy: bool,
    had_focus: bool,
    done: bool,
    skin: Skin,
    actions: Sender<Action>,
}

impl PowerMenu {
    pub fn new(skin: Skin, actions: Sender<Action>) -> Self {
        Self {
            abilities: None,
            asking: None,
            problem: None,
            busy: false,
            had_focus: false,
            done: false,
            skin,
            actions,
        }
    }

    pub fn close(&mut self) {
        self.done = true;
    }

    pub fn failed(&mut self, problem: String) {
        self.busy = false;
        self.asking = None;
        self.problem = Some(problem);
    }

    /// A row clicked: done at once, or asked about first.
    fn choose(&mut self, action: Action) {
        self.problem = None;
        if action.immediate() {
            self.carry_out(action);
        } else {
            self.asking = Some(action);
        }
    }

    fn carry_out(&mut self, action: Action) {
        self.busy = true;
        let _ = self.actions.send(action);
    }

    fn rows(&mut self, ui: &mut Ui, abilities: &Abilities) {
        let mut chosen = None;
        for action in Action::ALL {
            let possible = action.possible(abilities) && !self.busy;
            let (rect, response) = ui.allocate_exact_size(
                vec2(ui.available_width(), ROW),
                if possible {
                    Sense::click()
                } else {
                    Sense::hover()
                },
            );
            let v = ui.visuals().clone();
            if possible && response.hovered() {
                ui.painter().rect_filled(
                    rect,
                    v.widgets.hovered.corner_radius,
                    v.widgets.hovered.weak_bg_fill,
                );
            }
            let colour = if possible {
                v.text_color()
            } else {
                v.weak_text_color()
            };
            ui.painter().text(
                rect.left_center() + vec2(12.0, 0.0),
                egui::Align2::LEFT_CENTER,
                action.label(),
                egui::TextStyle::Body.resolve(ui.style()),
                colour,
            );
            if !action.possible(abilities) {
                response.on_hover_text(action.unavailable());
            } else if response.clicked() {
                chosen = Some(action);
            }
        }
        if let Some(action) = chosen {
            self.choose(action);
        }
    }

    fn ask(&mut self, ui: &mut Ui, action: Action) {
        let (question, answer) = action.question();
        ui.add_space(8.0);
        ui.label(RichText::new(question).strong());
        ui.label(RichText::new("Every app closes.").weak());
        ui.add_space(12.0);
        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            let go = ui.add_enabled(!self.busy, egui::Button::new(answer));
            if go.clicked() || (!self.busy && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                self.carry_out(action);
            }
            if ui.button("Cancel").clicked() {
                self.asking = None;
            }
        });
    }
}

impl Shell for PowerMenu {
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
            return;
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            // Escape takes back a question first, then closes the menu.
            if self.asking.take().is_none() {
                self.done = true;
                return;
            }
        }

        let rect = ui.max_rect();
        let v = ui.visuals().clone();
        ui.painter().rect(
            rect,
            v.menu_corner_radius,
            v.panel_fill,
            v.window_stroke,
            StrokeKind::Inside,
        );
        let mut ui = ui.new_child(
            UiBuilder::new()
                .max_rect(rect.shrink(MARGIN))
                .layout(Layout::top_down(Align::Min)),
        );
        ui.spacing_mut().item_spacing.y = 2.0;
        let Some(abilities) = self.abilities else {
            return;
        };
        match self.asking {
            Some(action) => self.ask(&mut ui, action),
            None => self.rows(&mut ui, &abilities),
        }
        if let Some(problem) = &self.problem {
            ui.label(RichText::new(problem).small().color(v.error_fg_color));
        }
    }

    fn finished(&self) -> bool {
        self.done
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;

    fn menu() -> (PowerMenu, mpsc::Receiver<Action>) {
        let (send, receive) = mpsc::channel();
        (PowerMenu::new(Skin::default_skin(), send), receive)
    }

    #[test]
    fn lock_is_at_once_and_the_rest_ask_first() {
        let (mut m, chosen) = menu();
        m.choose(Action::Lock);
        assert_eq!(chosen.try_recv(), Ok(Action::Lock));
        for action in [Action::LogOut, Action::Restart, Action::ShutDown] {
            m.choose(action);
            assert_eq!(m.asking, Some(action));
            assert!(chosen.try_recv().is_err(), "{action:?} went without asking");
        }
    }

    #[test]
    fn a_failure_is_shown_and_the_menu_can_be_used_again() {
        let (mut m, chosen) = menu();
        m.choose(Action::ShutDown);
        m.carry_out(Action::ShutDown);
        assert_eq!(chosen.try_recv(), Ok(Action::ShutDown));
        m.failed("Interactive authentication required".into());
        assert!(!m.busy);
        assert_eq!(m.asking, None);
        assert!(m.problem.is_some());
        m.choose(Action::Lock);
        assert_eq!(m.problem, None);
    }

    #[test]
    fn what_this_machine_can_do_decides_what_can_be_chosen() {
        let none = Abilities::default();
        assert!(Action::Lock.possible(&none));
        assert!(!Action::LogOut.possible(&none));
        assert!(!Action::ShutDown.possible(&none));
        let all = Abilities {
            session: true,
            restart: true,
            shut_down: true,
        };
        assert!(Action::ALL.iter().all(|a| a.possible(&all)));
    }
}
