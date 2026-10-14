//! `~/.config/patina-bar/config.toml`. Every key has a default, so the file
//! can be missing. A broken file never stops the bar: it falls back to the
//! defaults and says why on stderr, and unknown keys are warnings, as with
//! Patina skins.

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// The modules a side of the bar can hold.
pub const MODULES: &[&str] = &[
    "launcher", "windows", "tray", "tags", "layout", "title", "appid", "clock", "load",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Position {
    Top,
    Bottom,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    pub position: Position,
    /// Height in logical pixels.
    pub height: u32,
    /// A Patina skin by name: a built-in, or one in ~/.config/patina-bar/skins.
    pub skin: String,
    /// One label per dwl tag, in order. dwl has 9 by default.
    pub tags: Vec<String>,
    /// Show only tags that have windows or are being viewed.
    pub hide_vacant_tags: bool,
    pub left: Vec<String>,
    pub center: Vec<String>,
    pub right: Vec<String>,
    /// A strftime format, as in date(1).
    pub clock: String,
    /// What the launcher button runs, through sh(1).
    pub launcher: String,
    /// What the tray's speaker runs, through sh(1).
    pub volume: String,
    /// What the tray's Wi-Fi icon runs, through sh(1).
    pub wifi: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            position: Position::Bottom,
            // The height of a Patina window's header bar: its 25 px row,
            // 8 px margins and the 1 px line under it.
            height: 41,
            skin: patina::skin::DEFAULT_SKIN.to_string(),
            tags: (1..=9).map(|n| n.to_string()).collect(),
            hide_vacant_tags: false,
            left: vec![
                "launcher".into(),
                "windows".into(),
                "tags".into(),
                "layout".into(),
            ],
            center: vec!["title".into()],
            right: vec!["tray".into(), "load".into(), "clock".into()],
            clock: "%a %-d %b  %H:%M".into(),
            launcher: "patina-launcher".into(),
            volume: "patina-volume".into(),
            wifi: "patina-wifi".into(),
        }
    }
}

pub fn default_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("patina-bar").join("config.toml"))
}

/// Reads the config at `path`, returning it and anything worth telling the
/// user. A missing file is the defaults, silently.
pub fn load(path: &Path) -> (Config, Vec<String>) {
    match std::fs::read_to_string(path) {
        Ok(text) => parse(&text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Config::default(), Vec::new()),
        Err(e) => (
            Config::default(),
            vec![format!("{}: {e}; using the defaults", path.display())],
        ),
    }
}

pub fn parse(text: &str) -> (Config, Vec<String>) {
    let mut warnings = Vec::new();
    let table: toml::Table = match toml::from_str(text) {
        Ok(t) => t,
        Err(e) => return (Config::default(), vec![format!("{e}; using the defaults")]),
    };
    let config: Config = match serde_ignored::deserialize(toml::Value::Table(table), |path| {
        warnings.push(format!("unknown key `{path}`, ignored"))
    }) {
        Ok(c) => c,
        Err(e) => return (Config::default(), vec![format!("{e}; using the defaults")]),
    };
    for module in config
        .left
        .iter()
        .chain(&config.center)
        .chain(&config.right)
    {
        if !MODULES.contains(&module.as_str()) {
            warnings.push(format!(
                "unknown module `{module}`, left out; the modules are {}",
                MODULES.join(", ")
            ));
        }
    }
    (config, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_is_the_defaults() {
        let (c, w) = parse("");
        assert!(w.is_empty());
        assert_eq!(c.height, 41);
        assert_eq!(c.tags.len(), 9);
        assert_eq!(c.position, Position::Bottom);
    }

    #[test]
    fn reads_keys() {
        let (c, w) = parse(
            "position = \"top\"\nheight = 30\nright = [\"clock\"]\ntags = [\"web\", \"code\"]\n",
        );
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(c.position, Position::Top);
        assert_eq!(c.height, 30);
        assert_eq!(c.right, ["clock"]);
        assert_eq!(c.tags, ["web", "code"]);
        assert_eq!(c.left, Config::default().left);
    }

    #[test]
    fn unknown_keys_and_modules_are_warnings() {
        let (c, w) = parse("colour = \"red\"\nleft = [\"tags\", \"weather\"]\n");
        assert_eq!(c.left, ["tags", "weather"]);
        assert_eq!(w.len(), 2, "{w:?}");
        assert!(w[0].contains("colour"));
        assert!(w[1].contains("weather"));
    }

    #[test]
    fn a_bad_file_falls_back_to_the_defaults() {
        let (c, w) = parse("height = \"tall\"");
        assert_eq!(c.height, 41);
        assert_eq!(w.len(), 1);
        let (_, w) = parse("height = ");
        assert_eq!(w.len(), 1);
    }
}
