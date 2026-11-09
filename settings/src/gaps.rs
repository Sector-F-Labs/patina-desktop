//! Gaps, on the HUD page (ADR-0051): `[gaps]` in the compositor's
//! config, which the compositor follows for tiled windows and the bar for
//! floating, within a second.

use egui::{RichText, Ui};

use crate::app::Message;
use crate::compositor_config::{self as config, Gaps};

pub struct GapsSection {
    gaps: Gaps,
}

impl GapsSection {
    pub fn new() -> Self {
        Self {
            gaps: config::read_gaps(),
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, message: &mut Option<Message>) {
        ui.label(RichText::new("Gaps").strong());
        let mut save = ui
            .checkbox(
                &mut self.gaps.enabled,
                "Leave gaps around windows and the bar",
            )
            .changed();
        ui.label(
            RichText::new(
                "Tiled and maximised windows keep a gap around them and between them, and the \
                 bar floats off the screen's edge with rounded corners.",
            )
            .small()
            .weak(),
        );
        ui.add_space(4.0);
        ui.add_enabled_ui(self.gaps.enabled, |ui| {
            save |= ui
                .checkbox(
                    &mut self.gaps.maximise_inside,
                    "An app's maximise fills inside the gaps",
                )
                .changed();
            ui.label(
                RichText::new(
                    "Keeps the window's rounded corners and the gap, as Super+Up does. Turn it \
                     off for a real maximise: the window covers the whole screen, gaps and all.",
                )
                .small()
                .weak(),
            );
            ui.add_space(4.0);
            egui::Grid::new("gaps").num_columns(2).show(ui, |ui| {
                for (name, value) in [
                    ("Gap", &mut self.gaps.size),
                    ("Bar corners", &mut self.gaps.radius),
                ] {
                    ui.label(name);
                    let slider =
                        ui.add(egui::Slider::new(value, 0..=config::MOST_GAP).suffix(" px"));
                    // Saved when it's let go, not on every step of a drag.
                    save |= slider.drag_stopped() || (slider.changed() && !slider.dragged());
                    ui.end_row();
                }
            });
        });
        if save && let Err(e) = config::write_gaps(&self.gaps) {
            *message = Some(Message::error(format!("Couldn't save the gaps: {e}")));
        }
    }
}
