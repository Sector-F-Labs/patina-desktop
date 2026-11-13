//! `~/.config/patina-game-mode/config.toml` (ADR-0134): flat keys, all optional;
//! a missing or broken file is the defaults. Settings' Game mode page writes it,
//! and the daemon reads it again when the file changes.

use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Start by itself when a fullscreen window has the keyboard.
    pub enabled: bool,
    /// Seconds that has to last before it counts, and before it ends.
    pub delay: u64,
    /// Process names to ask to quit.
    pub kill: Vec<String>,
    /// Commands run through sh(1) on entering and on leaving.
    pub on_enter: Vec<String>,
    pub on_exit: Vec<String>,
}

/// The longest delay taken, in seconds.
pub const MOST_DELAY: u64 = 300;

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: false,
            delay: 3,
            kill: Vec::new(),
            on_enter: Vec::new(),
            on_exit: Vec::new(),
        }
    }
}

pub fn path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("patina-game-mode/config.toml"))
}

/// The config in `text`, and what's wrong with it. Unknown keys are left alone.
pub fn parse(text: &str) -> Result<(Config, Vec<String>), String> {
    let mut config: Config = toml::from_str(text).map_err(|e| e.to_string())?;
    let mut warnings = Vec::new();
    if config.delay > MOST_DELAY {
        warnings.push(format!(
            "delay {} is over {MOST_DELAY}; using {}",
            config.delay,
            Config::default().delay
        ));
        config.delay = Config::default().delay;
    }
    Ok((config, warnings))
}

/// The config now. A missing file is the defaults; a broken one is reported
/// and `previous` stays.
pub fn load(previous: &Config) -> Config {
    let Some(path) = path() else {
        return previous.clone();
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => match parse(&text) {
            Ok((config, warnings)) => {
                for w in warnings {
                    eprintln!("patina-game-mode: {w}");
                }
                config
            }
            Err(e) => {
                eprintln!(
                    "patina-game-mode: {}: {e}; keeping the settings in use",
                    path.display()
                );
                previous.clone()
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Config::default(),
        Err(e) => {
            eprintln!("patina-game-mode: {}: {e}", path.display());
            previous.clone()
        }
    }
}

pub fn modified() -> Option<std::time::SystemTime> {
    std::fs::metadata(path()?).ok()?.modified().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_the_defaults_and_keys_are_read() {
        assert_eq!(parse("").unwrap().0, Config::default());
        let (c, w) = parse(
            "enabled = true\ndelay = 3\nkill = [\"rust-analyzer\"]\non_enter = [\"docker stop $(docker ps -q)\"]\nunknown = 1\n",
        )
        .unwrap();
        assert!(c.enabled);
        assert_eq!(c.delay, 3);
        assert_eq!(c.kill, ["rust-analyzer"]);
        assert_eq!(c.on_enter, ["docker stop $(docker ps -q)"]);
        assert!(w.is_empty());
        assert!(parse("enabled = ").is_err());
    }

    #[test]
    fn a_long_delay_falls_back() {
        let (c, w) = parse("delay = 9999").unwrap();
        assert_eq!(c.delay, Config::default().delay);
        assert_eq!(w.len(), 1);
    }
}
