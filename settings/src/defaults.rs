//! The Defaults page (ADR-0079): the app that opens each kind of link and
//! file in the Patina session, for Files and every other app that opens
//! things through xdg-open, chosen from the installed apps that take it and
//! saved in `~/.config/patina/defaults.toml` by `patina_system::defaults`.

use egui::{RichText, Ui, vec2};
use patina_system::apps::{self, Entry};
use patina_system::defaults::ROLES;

use crate::app::Message;

pub struct DefaultsPage {
    /// The installed apps, read when the page is first shown.
    entries: Option<Vec<Entry>>,
    /// Each of `ROLES`' default, by app id.
    current: Vec<Option<String>>,
}

/// What `app` is called: its name, and its id as well where another
/// installed app has the same name (GNOME's Files and Patina's).
fn label(app: &Entry, entries: &[Entry]) -> String {
    if entries.iter().filter(|e| e.name == app.name).count() > 1 {
        format!("{} ({})", app.name, app.id)
    } else {
        app.name.clone()
    }
}

impl DefaultsPage {
    pub fn new() -> Self {
        Self {
            entries: None,
            current: Vec::new(),
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, message: &mut Option<Message>) {
        let entries = self.entries.get_or_insert_with(|| {
            let entries = apps::entries();
            self.current = ROLES.iter().map(|r| r.current(&entries)).collect();
            entries
        });
        ui.heading("Defaults");
        ui.label("The apps that open links and files, from Files and every other app.");
        ui.add_space(12.0);

        let mut chosen = None;
        egui::Grid::new("defaults")
            .num_columns(2)
            .spacing(vec2(16.0, 8.0))
            .show(ui, |ui| {
                for (i, role) in ROLES.iter().enumerate() {
                    ui.label(role.name);
                    let mut apps = role.apps(entries);
                    let current = self.current[i].as_deref();
                    // One chosen elsewhere that doesn't say it takes the type.
                    if let Some(app) = current
                        .and_then(|id| entries.iter().find(|e| e.id == id))
                        .filter(|app| !apps.iter().any(|a| a.id == app.id))
                    {
                        apps.push(app);
                    }
                    if apps.is_empty() {
                        ui.label(RichText::new("None installed").weak());
                        ui.end_row();
                        continue;
                    }
                    let shown = apps
                        .iter()
                        .find(|a| Some(a.id.as_str()) == current)
                        .map(|a| label(a, entries))
                        .unwrap_or_else(|| "Not chosen".to_string());
                    egui::ComboBox::from_id_salt(role.name)
                        .width(260.0)
                        .selected_text(shown)
                        .show_ui(ui, |ui| {
                            for app in &apps {
                                if ui
                                    .selectable_label(
                                        current == Some(app.id.as_str()),
                                        label(app, entries),
                                    )
                                    .on_hover_text(&app.id)
                                    .clicked()
                                {
                                    chosen = Some((i, *app));
                                }
                            }
                        });
                    ui.end_row();
                }
            });
        if let Some((i, app)) = chosen {
            match ROLES[i].set(app, entries) {
                Ok(()) => self.current[i] = Some(app.id.clone()),
                Err(e) => {
                    *message = Some(Message::error(format!("Couldn't save the default: {e}")))
                }
            }
        }

        ui.add_space(12.0);
        ui.label(
            RichText::new(
                "Saved in ~/.config/patina/defaults.toml, for this desktop only; others keep their own.",
            )
            .small()
            .weak(),
        );
    }
}
