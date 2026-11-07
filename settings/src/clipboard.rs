//! The Clipboard page (ADR-0114, ADR-0116): whether the compositor keeps a history of what's
//! copied, whether screenshots go on the clipboard, and how many copies are kept, as
//! `[clipboard]` in the compositor's config, which it follows within a second; and Clear
//! history, which asks it over the session bus.

use egui::{DragValue, RichText, Ui};

use crate::app::Message;
use crate::compositor_config::{self as config, CLIPBOARD_ENTRIES, Clipboard};

/// How long a copy is kept: what the page offers, in seconds; 0 is never.
const KEEP: [(u64, &str); 6] = [
    (0, "Until it's pushed out by newer ones"),
    (3600, "1 hour"),
    (6 * 3600, "6 hours"),
    (24 * 3600, "1 day"),
    (7 * 24 * 3600, "1 week"),
    (30 * 24 * 3600, "1 month"),
];

/// What `seconds` is called: from `KEEP`, or in the biggest whole unit it is.
fn keep_name(seconds: u64) -> String {
    if let Some((_, name)) = KEEP.iter().find(|(s, _)| *s == seconds) {
        return (*name).to_string();
    }
    let (n, unit) = match seconds {
        s if s % 86400 == 0 => (s / 86400, "day"),
        s if s % 3600 == 0 => (s / 3600, "hour"),
        s if s % 60 == 0 => (s / 60, "minute"),
        s => (s, "second"),
    };
    format!("{n} {unit}{}", if n == 1 { "" } else { "s" })
}

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

        ui.add_enabled_ui(self.clipboard.enabled, |ui| {
            ui.horizontal(|ui| {
                ui.label("Keep a copy for");
                egui::ComboBox::from_id_salt("clipboard-expire")
                    .selected_text(keep_name(self.clipboard.expire_after))
                    .show_ui(ui, |ui| {
                        // A value the file has that the list doesn't, kept as the first.
                        let mut choices = KEEP.map(|(s, n)| (s, n.to_string())).to_vec();
                        if !KEEP.iter().any(|(s, _)| *s == self.clipboard.expire_after) {
                            choices.insert(
                                0,
                                (
                                    self.clipboard.expire_after,
                                    keep_name(self.clipboard.expire_after),
                                ),
                            );
                        }
                        for (seconds, name) in choices {
                            changed |= ui
                                .selectable_value(&mut self.clipboard.expire_after, seconds, name)
                                .changed();
                        }
                    });
            });
        });
        ui.label(
            RichText::new(
                "Older copies are dropped, so something that slipped past the password \
                 check doesn't stay. One the desktop is holding as the clipboard stops pasting too; an app still holding its own isn't touched.",
            )
            .small()
            .weak(),
        );

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_are_named_in_words() {
        assert_eq!(keep_name(0), "Until it's pushed out by newer ones");
        assert_eq!(keep_name(3600), "1 hour");
        assert_eq!(keep_name(2 * 86400), "2 days");
        assert_eq!(keep_name(5 * 3600), "5 hours");
        assert_eq!(keep_name(90), "90 seconds");
        assert_eq!(keep_name(120), "2 minutes");
    }
}
