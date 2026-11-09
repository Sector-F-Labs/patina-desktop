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
            bar_buttons(ui);
            let visuals = ui.visuals().clone();
            let go = ui.add_enabled(
                !self.busy,
                egui::Button::new(RichText::new(answer).color(visuals.selection.stroke.color))
                    .fill(visuals.selection.bg_fill),
            );
            if go.clicked() || (!self.busy && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                self.carry_out(action);
            }
            if ui.button("Cancel").clicked() {
                self.asking = None;
            }
        });
    }
}

/// Buttons as big as a window's title bar and tool bar have them (patina-ui's
/// bar buttons): as wide as their text plus 10, 24 high, not egui's tiny ones.
fn bar_buttons(ui: &mut Ui) {
    let spacing = ui.spacing_mut();
    spacing.button_padding = vec2(10.0, 5.0);
    spacing.interact_size.y = 24.0;
    spacing.item_spacing.x = 8.0;
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

    /// Draws the menu and the question it asks in each built-in skin, as PNGs
    /// in the folder PATINA_POWER_PREVIEW names:
    /// `PATINA_POWER_PREVIEW=/tmp/p cargo test -p patina-power preview -- --ignored`.
    #[test]
    #[ignore]
    fn preview_the_menu() {
        let dir = std::path::PathBuf::from(std::env::var("PATINA_POWER_PREVIEW").unwrap());
        std::fs::create_dir_all(&dir).unwrap();
        let (skins, _) = patina::skin::load_all(patina::skin::BUILT_IN, &[]);
        let (w, h) = (crate::SIZE.0 as usize, crate::SIZE.1 as usize);
        for skin in skins {
            let name = skin.name.clone();
            for state in ["menu", "question"] {
                let (send, _chosen) = mpsc::channel();
                let mut m = PowerMenu::new(skin.clone(), send);
                m.abilities = Some(Abilities {
                    session: true,
                    restart: true,
                    shut_down: true,
                });
                m.had_focus = true;
                if state == "question" {
                    m.asking = Some(Action::LogOut);
                }
                let ctx = egui::Context::default();
                m.setup(&ctx);
                let mut painter = patina_shell::raster::Painter::default();
                let mut pixels = vec![0u32; w * h];
                for _ in 0..3 {
                    let input = egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::pos2(0.0, 0.0),
                            vec2(w as f32, h as f32),
                        )),
                        focused: true,
                        max_texture_side: Some(8192),
                        ..Default::default()
                    };
                    let out = ctx.run_ui(input, |ui| m.ui(ui, ""));
                    let primitives = ctx.tessellate(out.shapes, out.pixels_per_point);
                    painter.paint(
                        &mut pixels,
                        w,
                        h,
                        out.pixels_per_point,
                        &primitives,
                        &out.textures_delta,
                    );
                }
                let rgba: Vec<u8> = pixels
                    .iter()
                    .flat_map(|p| {
                        let [b, g, r, a] = p.to_le_bytes();
                        [r, g, b, a]
                    })
                    .collect();
                let file = dir.join(format!("power-{name}-{state}.png"));
                image::save_buffer(&file, &rgba, w as u32, h as u32, image::ColorType::Rgba8)
                    .unwrap();
            }
        }
    }
}
