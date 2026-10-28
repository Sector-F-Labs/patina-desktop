//! Settings kept between runs in `~/.config/draw/config.toml`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::bitmap::Fill;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub skin: Option<String>,
    pub system_title_bar: bool,
    /// Brush and shape stroke width, in pixels.
    pub size: u32,
    pub fill: Fill,
}

impl Default for Config {
    fn default() -> Self {
        Self { skin: None, system_title_bar: false, size: 4, fill: Fill::Outline }
    }
}

fn path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("draw").join("config.toml"))
}

impl Config {
    pub fn load() -> Self {
        path().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| toml::from_str(&t).ok()).unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(path) = path() else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = toml::to_string_pretty(self) {
            let _ = std::fs::write(path, text);
        }
    }
}
