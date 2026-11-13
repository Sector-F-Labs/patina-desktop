//! The Search page (ADR-0137): whether the launcher searches files, and
//! which folders it looks in and how: by name with fd, by what's in the files
//! with ripgrep, or both. It edits `~/.config/patina-search/config.toml`;
//! the launcher reads it each time it opens.

use egui::{RichText, Ui};
use patina_search::Tools;
use patina_search::config::{self, Config, Folder, Style};

use crate::app::Message;

pub struct SearchPage {
    config: Config,
    tools: Tools,
    /// The folder being typed to add.
    adding: String,
}

impl SearchPage {
    pub fn new() -> Self {
        Self {
            config: config::read(),
            tools: Tools::find(),
            adding: String::new(),
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, message: &mut Option<Message>) {
        ui.heading("Search");
        let mut changed = false;

        ui.add_space(8.0);
        changed |= ui
            .checkbox(&mut self.config.enabled, "Search files from the launcher")
            .changed();
        ui.label(
            RichText::new(
                "Typing in the launcher also lists matching files from the folders below, \
                 below the apps. Enter or a click opens one with its app.",
            )
            .small()
            .weak(),
        );

        ui.add_space(8.0);
        self.tool_notes(ui);

        ui.add_space(12.0);
        ui.add_enabled_ui(self.config.enabled, |ui| {
            ui.label(RichText::new("Folders").strong());
            ui.add_space(4.0);
            let mut remove = None;
            egui::Grid::new("search folders")
                .num_columns(3)
                .spacing(egui::vec2(12.0, 6.0))
                .show(ui, |ui| {
                    for (i, folder) in self.config.folders.iter_mut().enumerate() {
                        let exists = folder.dir().is_dir();
                        ui.horizontal(|ui| {
                            changed |= ui.checkbox(&mut folder.enabled, "").changed();
                            let mut name = RichText::new(&folder.path);
                            if !exists {
                                name = name.weak();
                            }
                            let label = ui.label(name);
                            if !exists {
                                label.on_hover_text("This folder isn't there");
                            }
                        });
                        egui::ComboBox::from_id_salt(("search style", i))
                            .selected_text(folder.style.label())
                            .width(260.0)
                            .show_ui(ui, |ui| {
                                for style in Style::ALL {
                                    changed |= ui
                                        .selectable_value(&mut folder.style, style, style.label())
                                        .changed();
                                }
                            });
                        if ui.small_button("Remove").clicked() {
                            remove = Some(i);
                        }
                        ui.end_row();
                    }
                });
            if let Some(i) = remove {
                self.config.folders.remove(i);
                changed = true;
            }
            if self.config.folders.is_empty() {
                ui.label(RichText::new("No folders: nothing is searched.").weak());
            }

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let field = ui.add(
                    egui::TextEdit::singleline(&mut self.adding)
                        .hint_text("A folder to add, such as ~/Projects")
                        .desired_width(300.0),
                );
                let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let add = ui
                    .add_enabled(!self.adding.trim().is_empty(), egui::Button::new("Add"))
                    .clicked();
                if (add || enter) && self.add_folder(message) {
                    changed = true;
                }
            });
            ui.add_space(4.0);
            ui.label(
                RichText::new(
                    "By name looks at the names of files and folders. By contents looks \
                     inside text files, up to 2 MB each, and shows the line it found. \
                     Hidden files and what a .gitignore leaves out are skipped.",
                )
                .small()
                .weak(),
            );
        });

        ui.add_space(12.0);
        if ui.button("Reset to the defaults").clicked() {
            self.config = Config::default();
            changed = true;
        }

        if changed && let Err(e) = config::write(&self.config) {
            *message = Some(Message::error(format!(
                "Couldn't save the search settings: {e}"
            )));
        }
    }

    /// Adds the folder typed, unless it's one already; true if it was.
    fn add_folder(&mut self, message: &mut Option<Message>) -> bool {
        let typed = self.adding.trim().trim_end_matches('/');
        let path = config::short(&config::expand(typed));
        if self.config.folders.iter().any(|f| f.path == path) {
            *message = Some(Message::note(format!("{path} is in the list already.")));
            return false;
        }
        if !config::expand(&path).is_dir() {
            *message = Some(Message::note(format!(
                "{path} isn't a folder yet; it's added, and searched once it is."
            )));
        }
        self.config.folders.push(Folder::new(path, Style::Names));
        self.adding.clear();
        true
    }

    /// Says which of fd and ripgrep aren't installed, as what they do then
    /// does nothing.
    fn tool_notes(&self, ui: &mut Ui) {
        let note = |ui: &mut Ui, text: &str| {
            ui.label(RichText::new(text).color(ui.visuals().warn_fg_color));
        };
        match &self.tools.fd {
            Some(fd) => {
                ui.label(
                    RichText::new(format!("fd: {}", fd.display()))
                        .small()
                        .weak(),
                );
            }
            None => note(
                ui,
                "fd isn't installed, so searching by name finds nothing. Install fd (on Ubuntu, fd-find).",
            ),
        }
        match &self.tools.rg {
            Some(rg) => {
                ui.label(
                    RichText::new(format!("ripgrep: {}", rg.display()))
                        .small()
                        .weak(),
                );
            }
            None => note(
                ui,
                "ripgrep isn't installed, so searching contents finds nothing. Install ripgrep.",
            ),
        }
    }
}
