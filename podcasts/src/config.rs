//! Settings kept between runs in `~/.config/podcasts/config.toml`, and
//! where you are in each episode, in `~/.local/share/podcasts/progress.json`.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Feed URLs, in the sidebar's order.
    pub subscriptions: Vec<String>,
    pub selected: Option<String>,
    /// The episode last played, as (feed URL, episode id), shown in the player at start-up.
    pub last: Option<(String, String)>,
    pub volume: f32,
    /// Whether the episode list shows ones already played.
    pub show_played: bool,
    pub skin: Option<String>,
    /// "light" or "dark" while the skin follows the desktop; anything else follows it.
    pub light_dark: String,
    pub system_title_bar: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            subscriptions: Vec::new(),
            selected: None,
            last: None,
            volume: 1.0,
            show_played: true,
            skin: None,
            light_dark: "system".into(),
            system_title_bar: false,
        }
    }
}

pub fn dir() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("podcasts"))
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
}

/// Where you are in an episode.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Progress {
    /// Seconds in.
    pub position: f64,
    pub played: bool,
}

/// By episode id.
#[derive(Debug, Default)]
pub struct Listening {
    pub episodes: HashMap<String, Progress>,
}

/// Within this many seconds of the end, an episode counts as played.
const NEAR_END: f64 = 30.0;

impl Listening {
    fn file() -> Option<PathBuf> {
        dirs::data_dir().map(|d| d.join("podcasts").join("progress.json"))
    }

    pub fn load() -> Self {
        let episodes = Self::file()
            .and_then(|f| std::fs::read_to_string(f).ok())
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        Self { episodes }
    }

    pub fn save(&self) {
        let Some(file) = Self::file() else { return };
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string(&self.episodes) {
            let _ = std::fs::write(file, text);
        }
    }

    pub fn get(&self, id: &str) -> Progress {
        self.episodes.get(id).copied().unwrap_or_default()
    }

    /// Where to start an episode: where you left it, or the start once it's played.
    pub fn resume_at(&self, id: &str) -> f64 {
        let p = self.get(id);
        if p.played { 0.0 } else { p.position }
    }

    /// Records the position, marking the episode played near its end.
    pub fn set_position(&mut self, id: &str, position: f64, duration: Option<f64>) {
        let near_end = duration.is_some_and(|d| d > NEAR_END * 2.0 && position > d - NEAR_END);
        let p = self.episodes.entry(id.to_string()).or_default();
        if near_end {
            *p = Progress { position: 0.0, played: true };
        } else {
            p.position = position;
            if position > 1.0 {
                p.played = false;
            }
        }
    }

    pub fn set_played(&mut self, id: &str, played: bool) {
        self.episodes.insert(id.to_string(), Progress { position: 0.0, played });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn near_the_end_counts_as_played() {
        let mut l = Listening::default();
        l.set_position("a", 120.0, Some(1800.0));
        assert_eq!(l.get("a"), Progress { position: 120.0, played: false });
        assert_eq!(l.resume_at("a"), 120.0);
        l.set_position("a", 1790.0, Some(1800.0));
        assert!(l.get("a").played);
        assert_eq!(l.resume_at("a"), 0.0);
    }

    #[test]
    fn listening_again_unmarks_played() {
        let mut l = Listening::default();
        l.set_played("a", true);
        l.set_position("a", 60.0, Some(1800.0));
        assert!(!l.get("a").played);
    }

    #[test]
    fn config_reads_a_partial_file() {
        let c: Config = toml::from_str("subscriptions = [\"https://a/rss\"]").unwrap();
        assert_eq!(c.subscriptions.len(), 1);
        assert_eq!(c.volume, 1.0);
    }
}
