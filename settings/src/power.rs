//! The Power page (ADR-0042): the power modes this machine has, through
//! power-profiles-daemon, with why performance is held back and which apps
//! are holding a mode.

use std::sync::mpsc;
use std::time::Duration;

use egui::{RichText, Ui};
use patina_system::power_profiles::{self, Status};

use crate::app::Message;

/// How often the daemon is asked how things stand, so a change made
/// elsewhere (a game, the battery running low) shows.
const POLL: Duration = Duration::from_secs(2);

pub struct PowerPage {
    /// What the daemon last said: `None` until it's answered.
    status: Option<Result<Option<Status>, String>>,
    updates: mpsc::Receiver<Result<Option<Status>, String>>,
    /// Results of switching, from their threads.
    switched: mpsc::Receiver<Result<(), String>>,
    switch: mpsc::Sender<Result<(), String>>,
}

impl PowerPage {
    pub fn new(ctx: &egui::Context) -> Self {
        let (switch, switched) = mpsc::channel();
        Self {
            status: None,
            updates: watch(ctx.clone()),
            switched,
            switch,
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
        ui.label("How the computer balances speed against battery life and heat.");
        ui.add_space(8.0);
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

        ui.label(RichText::new("Power Mode").strong());
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
