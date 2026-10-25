//! Settings, in `~/.config/text-editor/config.toml`: the skin, the frame,
//! how text is shown, and the files opened lately.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::indent::Indent;

/// The folder under `~/.config` and `~/.local/share`, and the skins'.
pub const APP: &str = "text-editor";
/// How many files Open Recent lists.
const RECENT: usize = 10;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub skin: String,
    /// "system", "light" or "dark", while the skin follows the desktop.
    pub light_dark: String,
    /// The desktop's title bar instead of Patina's.
    pub system_title_bar: bool,
    pub line_numbers: bool,
    pub wrap: bool,
    pub highlight_line: bool,
    pub font_size: f32,
    /// For new files and files with no indentation of their own.
    pub indent: Indent,
    pub recent: Vec<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            skin: patina::skin::DEFAULT_SKIN.into(),
            light_dark: "system".into(),
            system_title_bar: false,
            line_numbers: true,
            wrap: true,
            highlight_line: true,
            font_size: 14.0,
            indent: Indent::default(),
            recent: Vec::new(),
        }
    }
}

fn path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join(APP).join("config.toml"))
}

impl Config {
    pub fn load() -> Self {
        let Some(text) = path().and_then(|p| std::fs::read_to_string(p).ok()) else {
            return Config::default();
        };
        toml::from_str(&text).unwrap_or_else(|e| {
            eprintln!("text-editor: config.toml: {e}");
            Config::default()
        })
    }

    pub fn save(&self) {
        let Some(path) = path() else { return };
        if let Err(e) = write_toml(&path, self) {
            eprintln!("text-editor: can't save {}: {e}", path.display());
        }
    }

    pub fn theme(&self) -> egui::ThemePreference {
        match self.light_dark.as_str() {
            "light" => egui::ThemePreference::Light,
            "dark" => egui::ThemePreference::Dark,
            _ => egui::ThemePreference::System,
        }
    }

    /// Puts `path` first in Open Recent.
    pub fn opened(&mut self, path: &Path) {
        self.recent.retain(|p| p != path);
        self.recent.insert(0, path.to_path_buf());
        self.recent.truncate(RECENT);
    }
}

pub fn write_toml(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let text = toml::to_string(value).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(path.parent().unwrap_or(path)).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_fills_in_what_is_missing() {
        let mut config = Config { wrap: false, font_size: 16.0, ..Config::default() };
        config.opened(Path::new("/a"));
        config.opened(Path::new("/b"));
        config.opened(Path::new("/a"));
        assert_eq!(config.recent, [PathBuf::from("/a"), PathBuf::from("/b")]);
        let text = toml::to_string(&config).unwrap();
        assert_eq!(toml::from_str::<Config>(&text).unwrap(), config);
        assert!(toml::from_str::<Config>("skin = \"Default\"\n").unwrap().line_numbers);
    }
}
