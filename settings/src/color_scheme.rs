//! Style, on the Appearance page: light or dark (ADR-0087), set through
//! `patina_system::color_scheme`, where the how and why are.

use egui::{RichText, Ui};
use patina_system::color_scheme::{self, Scheme};

use crate::app::Message;

pub struct ColorSchemeSection {
    /// `None` where gsettings or the key is missing, so there's nothing to set.
    chosen: Option<Scheme>,
}

impl ColorSchemeSection {
    pub fn new() -> Self {
        Self {
            chosen: color_scheme::read(),
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, message: &mut Option<Message>) {
        ui.label(RichText::new("Style").strong());
        let Some(chosen) = &mut self.chosen else {
            ui.label(RichText::new("GNOME's color-scheme setting isn't installed.").weak());
            return;
        };
        let before = *chosen;
        ui.horizontal(|ui| {
            for scheme in Scheme::ALL {
                ui.selectable_value(chosen, scheme, scheme.name());
            }
        });
        ui.label(
            RichText::new("Light or dark, for apps that follow it; the skin above isn't changed.")
                .small()
                .weak(),
        );
        if *chosen != before
            && let Err(e) = color_scheme::write(*chosen)
        {
            *message = Some(Message::error(format!("Couldn't set the style: {e}")));
        }
    }
}
