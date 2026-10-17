//! The Bar page (ADR-0050): what the window tabs show, which edge the bar
//! sits on, and the bar's config file for everything else. Both settings are
//! top-level keys in the bar's config, which the bar follows within a
//! second.

use egui::{RichText, Ui};

use crate::app::Message;

/// The window tabs' choices: the config's value and what Settings calls it.
const WINDOW_TABS: [(&str, &str); 3] = [
    ("both", "Icons and titles"),
    ("icons", "Icons"),
    ("titles", "Titles"),
];

pub struct BarPage {
    window_tabs: String,
    top: bool,
}

/// The bar config's top-level `key`, as the file says now.
fn read(key: &str) -> Option<String> {
    let text = std::fs::read_to_string(patina_shell::bar::config_path()?).ok()?;
    patina_shell::bar::top_level(&text, key)
}

impl BarPage {
    pub fn new() -> Self {
        let mut page = Self {
            window_tabs: String::new(),
            top: false,
        };
        page.reload();
        page
    }

    /// Reads the settings again, after the file changed.
    pub fn reload(&mut self) {
        self.window_tabs = read("window_tabs")
            .filter(|v| WINDOW_TABS.iter().any(|(value, _)| v == value))
            .unwrap_or_else(|| "both".into());
        self.top = read("position").as_deref() == Some("top");
    }

    fn set(key: &str, value: &str, message: &mut Option<Message>) {
        if let Err(e) = patina_shell::bar::set(key, value) {
            *message = Some(Message::error(format!("Couldn't save the setting: {e}")));
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, message: &mut Option<Message>) {
        ui.heading("Bar");
        ui.add_space(8.0);

        ui.label(RichText::new("Window Tabs").strong());
        ui.label(
            RichText::new("What each open window's tab on the bar shows.")
                .small()
                .weak(),
        );
        for (value, name) in WINDOW_TABS {
            if ui.radio(self.window_tabs == value, name).clicked() && self.window_tabs != value {
                self.window_tabs = value.to_string();
                Self::set("window_tabs", value, message);
            }
        }
        ui.label(
            RichText::new("Icon-only tabs show the title on hover, and close with a middle click.")
                .small()
                .weak(),
        );
        ui.add_space(16.0);

        ui.label(RichText::new("Position").strong());
        for (top, name) in [(false, "Bottom of the screen"), (true, "Top of the screen")] {
            if ui.radio(self.top == top, name).clicked() && self.top != top {
                self.top = top;
                Self::set("position", if top { "top" } else { "bottom" }, message);
            }
        }
        ui.add_space(16.0);

        if let Some(path) = patina_shell::bar::config_path() {
            ui.label(RichText::new("Everything Else").strong());
            ui.label(
                RichText::new(
                    "The modules on each side, the clock's format and the commands the bar runs \
                     are in its config file.",
                )
                .small()
                .weak(),
            );
            if ui
                .button("Open bar settings")
                .on_hover_text(path.display().to_string())
                .clicked()
            {
                if !path.exists() {
                    Self::set("window_tabs", &self.window_tabs, message);
                }
                if let Err(e) = patina::open::path(&path) {
                    *message = Some(Message::error(format!(
                        "Couldn't open {}: {e}",
                        path.display()
                    )));
                }
            }
        }
    }
}
