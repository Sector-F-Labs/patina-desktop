//! The Notifications page (ADR-0141): whether notifications are shown at all and whether
//! they wait while a fullscreen window has the keyboard, written to
//! `~/.config/patina-notify/config.toml`, which patina-notify follows within a second. The
//! HUD's bell switches the first one too.

use egui::{RichText, Ui};
use patina_system::notifications::{self, Config};

use crate::app::Message;

pub struct NotificationsPage {
    config: Config,
}

impl NotificationsPage {
    pub fn new() -> Self {
        Self {
            config: notifications::read(),
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, message: &mut Option<Message>) {
        ui.heading("Notifications");
        let mut changed = false;

        ui.add_space(8.0);
        changed |= ui
            .checkbox(&mut self.config.enabled, "Show notifications")
            .changed();
        ui.label(
            RichText::new(
                "Notifications are cards down the right edge of the screen, one under \
                 another, and they stay until you close them with the X in their corner \
                 (unless the app gave one a time). Tiled windows make room for the column while it has \
                 cards. Turned off, nothing is shown and no space is kept; the HUD's bell \
                 switches this too.",
            )
            .small()
            .weak(),
        );

        ui.add_space(12.0);
        ui.add_enabled_ui(self.config.enabled, |ui| {
            changed |= ui
                .checkbox(
                    &mut self.config.hide_in_fullscreen,
                    "Hold them back while a window is fullscreen",
                )
                .changed();
        });
        ui.label(
            RichText::new(
                "While a game or a video is fullscreen, nothing shows and no space is \
                 kept; what arrived meanwhile shows when you leave it. Off, they show \
                 on top of the screen's edge as usual, though the fullscreen window \
                 still covers them.",
            )
            .small()
            .weak(),
        );

        if changed && let Err(e) = notifications::write(self.config) {
            *message = Some(Message::error(format!(
                "Couldn't save the notification settings: {e}"
            )));
        }
    }
}
