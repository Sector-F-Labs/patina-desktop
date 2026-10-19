//! Icons, on the Appearance page: the icon theme Files and GTK apps draw
//! with. For now it's GNOME's setting (`org.gnome.desktop.interface
//! icon-theme`, through gsettings), which they already read; a setting of
//! the desktop's own can take its place later. One that isn't installed,
//! as a removed desktop leaves behind, shows as Yaru, which Files then uses.

use std::path::{Path, PathBuf};

use egui::{RichText, Ui};

use crate::app::Message;

/// The theme shown when the setting names none that's installed.
const DEFAULT: &str = "Yaru";

/// An installed icon theme: its folder's name, which the setting holds,
/// and the name it gives itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    pub id: String,
    pub name: String,
}

pub struct IconsSection {
    themes: Vec<Theme>,
    chosen: String,
}

impl IconsSection {
    pub fn new() -> Self {
        let themes = installed(&icon_dirs());
        let set = read().filter(|t| themes.iter().any(|i| i.id == *t));
        Self {
            chosen: set.unwrap_or_else(|| DEFAULT.into()),
            themes,
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, message: &mut Option<Message>) {
        ui.label(RichText::new("Icons").strong());
        if self.themes.is_empty() {
            ui.label(RichText::new("No icon themes are installed.").weak());
            return;
        }
        let shown = self
            .themes
            .iter()
            .find(|t| t.id == self.chosen)
            .map_or(self.chosen.clone(), |t| t.name.clone());
        let before = self.chosen.clone();
        ui.horizontal(|ui| {
            ui.label("Icon theme");
            egui::ComboBox::from_id_salt("icon-theme")
                .selected_text(shown)
                .show_ui(ui, |ui| {
                    for theme in &self.themes {
                        ui.selectable_value(&mut self.chosen, theme.id.clone(), &theme.name);
                    }
                });
        });
        ui.label(
            RichText::new("What Files and GTK apps draw folders, files and apps with.")
                .small()
                .weak(),
        );
        if self.chosen != before
            && let Err(e) = write(&self.chosen)
        {
            *message = Some(Message::error(format!("Couldn't set the icons: {e}")));
        }
    }
}

const SCHEMA: &str = "org.gnome.desktop.interface";

fn read() -> Option<String> {
    let out = std::process::Command::new("gsettings")
        .args(["get", SCHEMA, "icon-theme"])
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    let value = String::from_utf8_lossy(&out.stdout);
    Some(value.trim().trim_matches('\'').to_string()).filter(|v| !v.is_empty())
}

fn write(theme: &str) -> Result<(), String> {
    let status = std::process::Command::new("gsettings")
        .args(["set", SCHEMA, "icon-theme", theme])
        .status()
        .map_err(|e| format!("gsettings: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("gsettings: {status}"))
    }
}

/// Where icon themes are, as the icon theme spec looks: `~/.icons`, then
/// `icons` in each XDG data directory.
fn icon_dirs() -> Vec<PathBuf> {
    let env = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
    let home = env("HOME").map(PathBuf::from);
    let data_home = env("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| home.as_ref().map(|h| h.join(".local/share")));
    let data_dirs = env("XDG_DATA_DIRS").unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    home.map(|h| h.join(".icons"))
        .into_iter()
        .chain(data_home.map(|d| d.join("icons")))
        .chain(std::env::split_paths(&data_dirs).map(|d| d.join("icons")))
        .collect()
}

/// The icon themes in `dirs`, by name, each once: folders whose
/// index.theme lists icon directories, so not cursor themes, nor hicolor,
/// the fallback every theme has, nor hidden ones.
fn installed(dirs: &[PathBuf]) -> Vec<Theme> {
    let mut themes: Vec<Theme> = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let id = entry.file_name().to_string_lossy().into_owned();
            if id == "hicolor" || themes.iter().any(|t| t.id == id) {
                continue;
            }
            if let Some(name) = theme_name(&entry.path().join("index.theme")) {
                themes.push(Theme { id, name });
            }
        }
    }
    themes.sort_by_cached_key(|t| t.name.to_lowercase());
    themes
}

/// An icon theme's name from its index.theme, if it's one to offer.
fn theme_name(index: &Path) -> Option<String> {
    let text = std::fs::read_to_string(index).ok()?;
    let mut name = None;
    let mut icons = false;
    let mut in_section = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_section = line == "[Icon Theme]";
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match (in_section, key.trim(), value.trim()) {
            (true, "Name", v) => name = Some(v.to_string()),
            (true, "Directories", v) if !v.is_empty() => icons = true,
            (true, "Hidden", "true") => return None,
            _ => {}
        }
    }
    name.filter(|_| icons)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offers_icon_themes_and_not_cursors_hicolor_or_hidden_ones() {
        let dir = std::env::temp_dir().join(format!("patina-icons-{}", std::process::id()));
        let theme = |id: &str, index: &str| {
            std::fs::create_dir_all(dir.join(id)).unwrap();
            std::fs::write(dir.join(id).join("index.theme"), index).unwrap();
        };
        theme("Yaru", "[Icon Theme]\nName=Yaru\nDirectories=16x16/apps\n");
        theme(
            "Adwaita",
            "[Icon Theme]\nName=Adwaita\nDirectories=scalable/apps\n",
        );
        theme(
            "DMZ-White",
            "[Icon Theme]\nName=DMZ (White)\nInherits=core\n",
        );
        theme(
            "hicolor",
            "[Icon Theme]\nName=Hicolor\nDirectories=16x16/apps\n",
        );
        theme(
            "secret",
            "[Icon Theme]\nName=Secret\nDirectories=a\nHidden=true\n",
        );
        std::fs::create_dir_all(dir.join("breeze-dark")).unwrap();
        let names: Vec<String> = installed(std::slice::from_ref(&dir))
            .into_iter()
            .map(|t| t.name)
            .collect();
        assert_eq!(names, ["Adwaita", "Yaru"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
