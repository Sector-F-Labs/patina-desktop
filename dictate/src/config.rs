//! Dictation's settings, `$XDG_CONFIG_HOME/patina-dictate/config.toml`,
//! which Settings' Dictation page writes. Read at each press, so a change
//! is used at once; missing or broken, the defaults.

use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Config {
    /// The screen edge the overlay shows at while dictating.
    pub overlay: Edge,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Edge {
    #[default]
    Bottom,
    Top,
}

impl Edge {
    pub fn name(self) -> &'static str {
        match self {
            Edge::Bottom => "bottom",
            Edge::Top => "top",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "bottom" => Some(Edge::Bottom),
            "top" => Some(Edge::Top),
            _ => None,
        }
    }
}

pub fn path() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config"))
        .join("patina-dictate/config.toml")
}

/// The settings, or the defaults if there's no file or it's broken.
pub fn load() -> Config {
    let path = path();
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Config::default();
    };
    parse(&text).unwrap_or_else(|e| {
        eprintln!("patina-dictate: {}: {e}; using the defaults", path.display());
        Config::default()
    })
}

fn parse(text: &str) -> Result<Config, toml::de::Error> {
    toml::from_str(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_overlay_is_at_the_bottom_unless_set() {
        assert_eq!(parse("").unwrap().overlay, Edge::Bottom);
        assert_eq!(parse("overlay = \"top\"").unwrap().overlay, Edge::Top);
        assert!(parse("overlay = \"left\"").is_err());
        assert_eq!(Edge::from_name(Edge::Top.name()), Some(Edge::Top));
    }
}
