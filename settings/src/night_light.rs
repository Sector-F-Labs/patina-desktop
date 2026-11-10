//! The Night Light page (ADR-0039): the mode, the times and the warmth,
//! written into the compositor's config, and what the compositor says over
//! D-Bus: whether it can warm this screen, whether it's on, and who's
//! holding it off.

use std::sync::mpsc;
use std::time::Duration;

use egui::{RichText, Ui};
use patina_system::night_light::{self, Status};

use crate::app::Message;
use crate::compositor_config::{self as config, Mode, NightLight};

/// How often the compositor is asked how things stand.
const POLL: Duration = Duration::from_secs(1);

pub struct NightLightPage {
    settings: NightLight,
    /// What the compositor last said: `None` until it's answered, then
    /// `None` inside when nothing serves Night Light.
    status: Option<Result<Option<Status>, String>>,
    updates: mpsc::Receiver<Result<Option<Status>, String>>,
}

impl NightLightPage {
    pub fn new(ctx: &egui::Context) -> Self {
        Self {
            settings: config::read(),
            status: None,
            updates: watch(ctx.clone()),
        }
    }

    fn save(&mut self, message: &mut Option<Message>) {
        if let Err(e) = config::write(&self.settings) {
            *message = Some(Message::error(format!("Couldn't save Night Light: {e}")));
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, message: &mut Option<Message>) {
        while let Ok(status) = self.updates.try_recv() {
            self.status = Some(status);
        }
        ui.heading("Night Light");
        ui.label("Warms the screen's colours in the evening, which is easier on the eyes.");
        ui.add_space(8.0);
        self.state(ui, message);
        ui.add_space(12.0);

        let before = self.settings;
        ui.radio_value(&mut self.settings.mode, Mode::Off, "Off");
        ui.radio_value(&mut self.settings.mode, Mode::Always, "Always on");
        ui.radio_value(&mut self.settings.mode, Mode::Schedule, "On a schedule");
        ui.add_enabled_ui(self.settings.mode == Mode::Schedule, |ui| {
            ui.horizontal(|ui| {
                ui.add_space(24.0);
                ui.label("From");
                time_field(ui, &mut self.settings.from);
                ui.label("to");
                time_field(ui, &mut self.settings.to);
            });
        });
        let times_changed = self.settings != before;

        ui.add_space(12.0);
        let mut slid = false;
        ui.add_enabled_ui(self.settings.mode != Mode::Off, |ui| {
            ui.horizontal(|ui| {
                ui.label("Warmth");
                let slider = ui.add(
                    egui::Slider::new(
                        &mut self.settings.temperature,
                        // Neutral on the left, warmer to the right.
                        std::ops::RangeInclusive::new(config::NEUTRAL, config::WARMEST),
                    )
                    .suffix(" K")
                    .step_by(100.0),
                );
                // Saved when it's let go, not on every step of a drag.
                slid = slider.drag_stopped() || (slider.changed() && !slider.dragged());
            });
            ui.label(
                RichText::new("6500 K is the screen as it is; lower is warmer.")
                    .small()
                    .weak(),
            );
        });

        ui.add_space(12.0);
        let before = self.settings.pause_in_fullscreen;
        ui.add_enabled(
            self.settings.mode != Mode::Off,
            egui::Checkbox::new(
                &mut self.settings.pause_in_fullscreen,
                "Off while a game or video is fullscreen",
            ),
        );
        ui.label(
            RichText::new("The colours go back to normal for as long as it has the screen.")
                .small()
                .weak(),
        );
        let paused_changed = self.settings.pause_in_fullscreen != before;
        if times_changed || slid || paused_changed {
            self.save(message);
        }
    }

    /// What the compositor says: whether this screen can be warmed, and
    /// how things stand, with the override and inhibitors.
    fn state(&mut self, ui: &mut Ui, message: &mut Option<Message>) {
        let status = match &self.status {
            None => return,
            Some(Err(e)) => {
                ui.label(RichText::new(format!("Can't reach the compositor: {e}")).weak());
                return;
            }
            Some(Ok(None)) => {
                ui.label(
                    RichText::new(
                        "Night Light works when Patina runs on the hardware. Running in \
                         another desktop, or in a virtual machine, the machine's own night \
                         light applies.",
                    )
                    .weak(),
                );
                return;
            }
            Some(Ok(Some(status))) => status.clone(),
        };
        if !status.available {
            ui.label(
                RichText::new("This screen's driver has no gamma ramp, so it can't be warmed.")
                    .weak(),
            );
            return;
        }
        ui.horizontal(|ui| {
            let now = if status.active {
                format!("On now, at {} K.", status.temperature)
            } else {
                "Off now.".to_string()
            };
            ui.label(now);
            let schedule = self.settings.mode == Mode::Schedule;
            let action = match (status.overridden.as_str(), status.active) {
                ("auto", true) if schedule => Some((
                    format!("Turn off until {}", config::format_time(self.settings.to)),
                    "off",
                )),
                ("auto", true) => Some(("Turn off for now".to_string(), "off")),
                ("auto", false) if schedule && status.inhibitors.is_empty() => Some((
                    format!("Turn on until {}", config::format_time(self.settings.from)),
                    "on",
                )),
                ("auto", false) => None,
                _ => Some(("Back to the schedule".to_string(), "auto")),
            };
            if let Some((label, state)) = action
                && ui.button(label).clicked()
                && let Err(e) = night_light::set_override(state)
            {
                *message = Some(Message::error(format!("Couldn't change Night Light: {e}")));
            }
        });
        if !status.inhibitors.is_empty() {
            ui.label(
                RichText::new(format!("Held off by: {}.", status.inhibitors.join(", "))).weak(),
            );
        }
    }
}

/// An "HH:MM" pair of fields.
fn time_field(ui: &mut Ui, (h, m): &mut (u32, u32)) {
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        ui.add(
            egui::DragValue::new(h)
                .range(0..=23)
                .custom_formatter(|v, _| format!("{v:02}")),
        );
        ui.label(":");
        ui.add(
            egui::DragValue::new(m)
                .range(0..=59)
                .speed(0.5)
                .custom_formatter(|v, _| format!("{v:02}")),
        );
    });
}

/// Asks the compositor how Night Light stands every second, on a thread,
/// sending what changes.
fn watch(ctx: egui::Context) -> mpsc::Receiver<Result<Option<Status>, String>> {
    let (send, receive) = mpsc::channel();
    let _ = std::thread::Builder::new()
        .name("night-light".into())
        .spawn(move || {
            let mut last = None;
            loop {
                let now = night_light::status();
                if last.as_ref() != Some(&now) {
                    if send.send(now.clone()).is_err() {
                        return;
                    }
                    ctx.request_repaint();
                    last = Some(now);
                }
                std::thread::sleep(POLL);
            }
        });
    receive
}
