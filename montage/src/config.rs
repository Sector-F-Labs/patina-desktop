//! Settings kept between runs, in `~/.config/montage/config.toml`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub skin: Option<String>,
    /// "light" or "dark" while the skin follows the desktop; anything else follows it.
    pub light_dark: String,
    pub system_title_bar: bool,
    /// The project open when Montage last closed, opened again at start.
    pub project: Option<PathBuf>,
    /// The size last exported at, by name ("1080p").
    pub export_size: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            skin: None,
            light_dark: "system".into(),
            system_title_bar: false,
            project: None,
            export_size: "1080p".into(),
        }
    }
}

/// The config folder's name, and so the skins folder's.
pub const APP: &str = "montage";

fn path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join(APP).join("config.toml"))
}

impl Config {
    pub fn theme(&self) -> egui::ThemePreference {
        match self.light_dark.as_str() {
            "light" => egui::ThemePreference::Light,
            "dark" => egui::ThemePreference::Dark,
            _ => egui::ThemePreference::System,
        }
    }

    pub fn load() -> Self {
        path().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| toml::from_str(&t).ok()).unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(path) = path() else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = toml::to_string(self) {
            let _ = std::fs::write(path, text);
        }
    }
}
