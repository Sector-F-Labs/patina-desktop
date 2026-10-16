//! Reader's settings, in `~/.config/reader/config.toml`: the skin, the
//! frame, and the tabs open last, each a project (or the manual) and the
//! file it was on, to open again.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub skin: String,
    /// "system", "light" or "dark", while the skin follows the desktop.
    pub light_dark: String,
    /// The desktop's title bar instead of Patina's.
    pub system_title_bar: bool,
    pub tabs: Vec<SavedTab>,
    /// The tab that was in front.
    pub tab: usize,
    /// Before tabs: the one project read last, opened as the first tab.
    pub last_root: Option<PathBuf>,
    pub last_file: Option<PathBuf>,
}

/// A tab as it's remembered.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedTab {
    /// The project, or none for the manual or a new tab.
    pub root: Option<PathBuf>,
    /// The system's man pages rather than a project.
    pub man: bool,
    pub file: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            // Notebook's skin: documents look as they do there.
            skin: "Notebook".into(),
            light_dark: "system".into(),
            system_title_bar: false,
            tabs: Vec::new(),
            tab: 0,
            last_root: None,
            last_file: None,
        }
    }
}

fn path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("reader/config.toml"))
}

impl Config {
    pub fn load() -> Self {
        let Some(text) = path().and_then(|p| std::fs::read_to_string(p).ok()) else {
            return Config::default();
        };
        toml::from_str(&text).unwrap_or_else(|e| {
            eprintln!("reader: config.toml: {e}");
            Config::default()
        })
    }

    pub fn save(&self) {
        let Some(path) = path() else { return };
        let saved = toml::to_string(self).map_err(|e| e.to_string()).and_then(|text| {
            std::fs::create_dir_all(path.parent().unwrap_or(&path)).map_err(|e| e.to_string())?;
            std::fs::write(&path, text).map_err(|e| e.to_string())
        });
        if let Err(e) = saved {
            eprintln!("reader: can't save {}: {e}", path.display());
        }
    }

    pub fn theme(&self) -> egui::ThemePreference {
        match self.light_dark.as_str() {
            "light" => egui::ThemePreference::Light,
            "dark" => egui::ThemePreference::Dark,
            _ => egui::ThemePreference::System,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_fills_in_what_is_missing() {
        let config = Config {
            tabs: vec![
                SavedTab { root: Some("/p".into()), man: false, file: Some("/p/README.md".into()) },
                SavedTab { root: None, man: true, file: None },
            ],
            tab: 1,
            ..Config::default()
        };
        let text = toml::to_string(&config).unwrap();
        assert_eq!(toml::from_str::<Config>(&text).unwrap(), config);
        assert_eq!(toml::from_str::<Config>("skin = \"Default\"\n").unwrap().light_dark, "system");
    }
}
