//! The Power page: when the screen turns off without input (ADR-0047), and
//! the power modes this machine has (ADR-0042), through power-profiles-daemon,
//! with why performance is held back and which apps are holding a mode.

use std::sync::mpsc;
use std::time::Duration;

use egui::{RichText, Ui};
use patina_system::power_profiles::{self, Status};

use crate::app::Message;
use crate::compositor_config::{self as config, Screen};

/// How often the daemon is asked how things stand, so a change made
/// elsewhere (a game, the battery running low) shows.
const POLL: Duration = Duration::from_secs(2);
/// When the screen can turn off, as GNOME offers it: never, or minutes.
const SCREEN_OFF: [u64; 10] = [0, 60, 120, 180, 240, 300, 480, 600, 720, 900];

pub struct PowerPage {
    /// What the daemon last said: `None` until it's answered.
    status: Option<Result<Option<Status>, String>>,
    updates: mpsc::Receiver<Result<Option<Status>, String>>,
    /// Results of switching, from their threads.
    switched: mpsc::Receiver<Result<(), String>>,
    switch: mpsc::Sender<Result<(), String>>,
    screen: Screen,
}

impl PowerPage {
    pub fn new(ctx: &egui::Context) -> Self {
        let (switch, switched) = mpsc::channel();
        Self {
            status: None,
            updates: watch(ctx.clone()),
            switched,
            switch,
            screen: config::read_screen(),
        }
    }

    /// Switches on a thread, so a slow bus never holds the window up.
    fn choose(&mut self, profile: &str, ctx: &egui::Context) {
        if let Some(Ok(Some(status))) = &mut self.status {
            status.active = profile.to_string();
        }
        let (profile, send, ctx) = (profile.to_string(), self.switch.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = send.send(power_profiles::set(&profile));
            ctx.request_repaint();
        });
    }

    pub fn ui(&mut self, ui: &mut Ui, message: &mut Option<Message>) {
        while let Ok(status) = self.updates.try_recv() {
            self.status = Some(status);
        }
        while let Ok(result) = self.switched.try_recv() {
            if let Err(e) = result {
                *message = Some(Message::error(format!(
                    "Couldn't change the power mode: {e}"
                )));
            }
        }
        ui.heading("Power");
        ui.add_space(8.0);
        self.screen(ui, message);
        ui.add_space(16.0);
        self.modes(ui);
    }

    /// When the screen turns off, and whether that locks it.
    fn screen(&mut self, ui: &mut Ui, message: &mut Option<Message>) {
        ui.label(RichText::new("Screen").strong());
        let before = self.screen;
        ui.horizontal(|ui| {
            ui.label("Turn the screen off after");
            egui::ComboBox::from_id_salt("screen-off")
                .selected_text(describe(self.screen.off_after))
                .show_ui(ui, |ui| {
                    let mut choices = SCREEN_OFF.to_vec();
                    // One set in the file by hand is offered too.
                    if !choices.contains(&self.screen.off_after) {
                        choices.push(self.screen.off_after);
                        choices.sort();
                    }
                    for choice in choices {
                        ui.selectable_value(&mut self.screen.off_after, choice, describe(choice));
                    }
                });
        });
        ui.add_enabled_ui(self.screen.off_after != 0, |ui| {
            ui.checkbox(&mut self.screen.lock, "Lock the screen when it turns off");
        });
        ui.label(
            RichText::new(
                "Any key or movement turns it back on. Videos, and games through Steam, keep \
                 it on while they play.",
            )
            .small()
            .weak(),
        );
        if self.screen != before
            && let Err(e) = config::write_screen(&self.screen)
        {
            *message = Some(Message::error(format!("Couldn't save the setting: {e}")));
        }
    }

    /// The power modes, through power-profiles-daemon.
    fn modes(&mut self, ui: &mut Ui) {
        ui.label(RichText::new("Power Mode").strong());
        ui.label(
            RichText::new("How the computer balances speed against battery life and heat.")
                .small()
                .weak(),
        );
        let status = match &self.status {
            None => return,
            Some(Err(e)) => {
                ui.label(RichText::new(format!("Can't ask about power modes: {e}")).weak());
                return;
            }
            Some(Ok(None)) => {
                ui.label(
                    RichText::new(
                        "Power modes need power-profiles-daemon, which isn't installed here \
                         (apt install power-profiles-daemon on Ubuntu).",
                    )
                    .weak(),
                );
                return;
            }
            Some(Ok(Some(status))) => status.clone(),
        };

        let mut chosen = None;
        // Fastest first, as GNOME lists them.
        for profile in status.profiles.iter().rev() {
            let (name, about) = power_profiles::describe(profile);
            ui.horizontal(|ui| {
                if ui.radio(status.active == *profile, name).clicked() && status.active != *profile
                {
                    chosen = Some(profile.clone());
                }
                ui.label(RichText::new(about).weak());
            });
        }
        if let Some(profile) = chosen {
            self.choose(&profile, ui.ctx());
        }
        if !status.profiles.iter().any(|p| p == "performance") {
            ui.add_space(4.0);
            ui.label(
                RichText::new("This computer's driver doesn't offer a performance mode.")
                    .small()
                    .weak(),
            );
        }
        let degraded = power_profiles::degraded_reason(&status.degraded);
        if !degraded.is_empty() {
            ui.add_space(8.0);
            ui.colored_label(ui.visuals().warn_fg_color, degraded);
        }
        if !status.holds.is_empty() {
            ui.add_space(8.0);
            for hold in &status.holds {
                let mode = power_profiles::describe(&hold.profile).0;
                let app = if hold.app.is_empty() {
                    "An app"
                } else {
                    hold.app.as_str()
                };
                let why = if hold.reason.is_empty() {
                    String::new()
                } else {
                    format!(": {}", hold.reason)
                };
                ui.label(RichText::new(format!("{app} is holding {mode}{why}.")).weak());
            }
        }
    }
}

/// "Never", "1 minute", "5 minutes", or seconds for a time set by hand.
fn describe(seconds: u64) -> String {
    match seconds {
        0 => "Never".into(),
        60 => "1 minute".into(),
        s if s % 60 == 0 => format!("{} minutes", s / 60),
        s => format!("{s} seconds"),
    }
}

/// Asks the daemon how things stand every few seconds, on a thread,
/// sending what changes.
fn watch(ctx: egui::Context) -> mpsc::Receiver<Result<Option<Status>, String>> {
    let (send, receive) = mpsc::channel();
    let _ = std::thread::Builder::new()
        .name("power-profiles".into())
        .spawn(move || {
            let mut last = None;
            loop {
                let now = power_profiles::status();
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
