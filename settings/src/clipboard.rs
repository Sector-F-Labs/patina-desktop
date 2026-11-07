//! The Clipboard page (ADR-0114, ADR-0116): whether the compositor keeps a history of what's
//! copied, whether screenshots go on the clipboard, and how many copies are kept, as
//! `[clipboard]` in the compositor's config, which it follows within a second; and Clear
//! history, which asks it over the session bus.

use egui::{DragValue, RichText, Ui};

use crate::app::Message;
use crate::compositor_config::{self as config, CLIPBOARD_ENTRIES, Clipboard};

pub struct ClipboardPage {
    clipboard: Clipboard,
}

impl ClipboardPage {
    pub fn new() -> Self {
        Self {
            clipboard: config::read_clipboard(),
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, message: &mut Option<Message>) {
        ui.heading("Clipboard");
        let mut changed = false;

        ui.add_space(8.0);
        changed |= ui
            .checkbox(
                &mut self.clipboard.enabled,
                "Keep a history of what's copied",
            )
            .changed();
        ui.label(
            RichText::new(
                "The desktop keeps copies after the app they came from closes, and the \
                 clipboard icon on the HUD lists them to copy again. Turning it off drops \
                 what was kept. Copies marked as a password manager's are never kept.",
            )
            .small()
            .weak(),
        );
        ui.add_enabled_ui(self.clipboard.enabled, |ui| {
            ui.horizontal(|ui| {
                ui.label("Copies kept");
                changed |= ui
                    .add(DragValue::new(&mut self.clipboard.max_entries).range(CLIPBOARD_ENTRIES))
                    .changed();
            });
        });

        ui.add_space(12.0);
        changed |= ui
            .checkbox(
                &mut self.clipboard.screenshots,
                "Put screenshots on the clipboard",
            )
            .changed();
        ui.label(
            RichText::new(
                "A screenshot is saved as a file, and also copied, so it pastes straight \
                 away. This works with the history off too, and replaces what was copied.",
            )
            .small()
            .weak(),
        );

        ui.add_space(12.0);
        if ui
            .add_enabled(self.clipboard.enabled, egui::Button::new("Clear history"))
            .clicked()
        {
            // A call to the compositor on this machine's bus, answered at once.
            *message = Some(match patina_system::clipboard::clear() {
                Ok(()) => Message::note("Cleared the clipboard history."),
                Err(e) => Message::error(format!("Couldn't clear the history: {e}")),
            });
        }

        if changed && let Err(e) = config::write_clipboard(&self.clipboard) {
            *message = Some(Message::error(format!("Couldn't save the clipboard: {e}")));
        }
    }
}
