//! Shadows, on the Appearance page (ADR-0112): `[shadows]` in the compositor's
//! config, which the compositor follows within a second. It draws the shadow
//! a window asks for, as Patina apps do; apps that draw their own keep it.

use egui::{RichText, Ui};

use crate::app::Message;
use crate::compositor_config::{self as config, Shadows};

pub struct ShadowsSection {
    shadows: Shadows,
}

impl ShadowsSection {
    pub fn new() -> Self {
        Self {
            shadows: config::read_shadows(),
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, message: &mut Option<Message>) {
        ui.label(RichText::new("Shadows").strong());
        let changed = ui
            .checkbox(&mut self.shadows.enabled, "Draw shadows around windows")
            .changed();
        ui.label(
            RichText::new(
                "Patina apps ask the desktop for a soft shadow, lighter while the window isn't \
                 focused. Apps that draw their own, such as Firefox, keep theirs.",
            )
            .small()
            .weak(),
        );
        if changed && let Err(e) = config::write_shadows(&self.shadows) {
            *message = Some(Message::error(format!("Couldn't save the shadows: {e}")));
        }
    }
}
