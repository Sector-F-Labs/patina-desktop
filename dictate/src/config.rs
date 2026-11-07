//! Dictation's settings, `$XDG_CONFIG_HOME/patina-dictate/config.toml`,
//! which Settings' Dictation page writes. Read at each press, so a change
//! is used at once; missing or broken, the defaults.

use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Config {
    /// The screen edge the overlay shows at while dictating.
    pub overlay: Edge,
    /// whisper.cpp's name for the model (`base.en`, `small`), or a path to
    /// one; `None` for the default. A command-line `-m` wins over it.
    pub model: Option<String>,
    /// What's spoken (`en`, `sv`), or `auto`; `None` for English with an
    /// English-only model, else auto. A command-line `-l` wins over it.
    pub language: Option<String>,
    /// How many candidates the decoder weighs: 1 is greedy (fastest), more
    /// is slower and steadier on mumbling and accents. 1 to 8.
    pub beam_size: u32,
    /// Segments likelier than this to be no speech are dropped (0 to 1).
    pub no_speech: f32,
    /// Words Whisper is told came before: names, jargon, the spelling you
    /// want. Biases it towards them.
    pub prompt: String,
    /// Decoder threads; 0 for as many as there are, up to eight.
    pub threads: u32,
    /// Retry a hesitant segment hotter (temperature fallback) instead of
    /// accepting its first try.
    pub fallback: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            overlay: Edge::default(),
            model: None,
            language: None,
            beam_size: 1,
            no_speech: 0.6,
            prompt: String::new(),
            threads: 0,
            fallback: true,
        }
    }
}

impl Config {
    /// The values clamped to what whisper.cpp takes.
    pub fn sane(mut self) -> Self {
        self.beam_size = self.beam_size.clamp(1, 8);
        self.no_speech = if self.no_speech.is_finite() {
            self.no_speech.clamp(0.0, 1.0)
        } else {
            0.6
        };
        self.threads = self.threads.min(64);
        self.model = self.model.filter(|m| !m.trim().is_empty());
        self.language = self.language.filter(|l| !l.trim().is_empty());
        self
    }
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
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
        })
        .join("patina-dictate/config.toml")
}

/// The settings, or the defaults if there's no file or it's broken.
pub fn load() -> Config {
    let path = path();
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Config::default();
    };
    parse(&text).unwrap_or_else(|e| {
        eprintln!(
            "patina-dictate: {}: {e}; using the defaults",
            path.display()
        );
        Config::default()
    })
}

fn parse(text: &str) -> Result<Config, toml::de::Error> {
    toml::from_str::<Config>(text).map(Config::sane)
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

    #[test]
    fn the_tuning_has_defaults_and_limits() {
        let c = parse("").unwrap();
        assert_eq!((c.beam_size, c.threads, c.fallback), (1, 0, true));
        assert_eq!(c.model, None);
        let c =
            parse("model = \"small\"\nbeam_size = 99\nno_speech = 3.0\nlanguage = \"\"").unwrap();
        assert_eq!(c.model.as_deref(), Some("small"));
        assert_eq!((c.beam_size, c.no_speech, c.language), (8, 1.0, None));
    }
}
