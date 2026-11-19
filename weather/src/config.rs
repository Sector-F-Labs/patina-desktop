//! Settings kept between runs in `~/.config/weather/config.toml`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::forecast::Place;
use crate::units::Units;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum View {
    /// Now, the next hours and the days ahead, as macOS Weather opens.
    #[default]
    Overview,
    /// The days side by side with graphs across all of them.
    Week,
    /// One day in detail.
    Day,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub places: Vec<Place>,
    pub selected: usize,
    pub view: View,
    /// How many days the forecast list and the week view show: 7 or 14.
    pub days: usize,
    pub units: Units,
    pub skin: Option<String>,
    /// "light" or "dark" while the skin follows the desktop; anything else follows it.
    pub light_dark: String,
    pub system_title_bar: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            places: Vec::new(),
            selected: 0,
            view: View::Overview,
            days: 7,
            units: Units::default(),
            skin: None,
            light_dark: "system".into(),
            system_title_bar: false,
        }
    }
}

pub fn dir() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("weather"))
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
        dir()
            .and_then(|d| std::fs::read_to_string(d.join("config.toml")).ok())
            .and_then(|t| toml::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(dir) = dir() else { return };
        let _ = std::fs::create_dir_all(&dir);
        if let Ok(text) = toml::to_string(self) {
            let _ = std::fs::write(dir.join("config.toml"), text);
        }
    }

    pub fn place(&self) -> Option<&Place> {
        self.places.get(self.selected).or(self.places.first())
    }
}
