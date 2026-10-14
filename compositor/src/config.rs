//! `~/.config/patina-compositor/config.toml` (ADR-0019). Every key is
//! optional; a missing file is the defaults. A broken file never stops the
//! session: at start it falls back to the defaults, and on a reload
//! (Super+Shift+R) the settings in use stay. Unknown keys are warnings, as
//! in patina-bar's config and Patina's skins.

use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Config {
    /// The output's scale; unset, chosen from the panel's density (or the
    /// parent desktop's scale when nested). `PATINA_SCALE` overrides it.
    pub scale: Option<f64>,
    pub keyboard: Keyboard,
    pub touchpad: Touchpad,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Keyboard {
    /// xkb names; an unset layout means the system's (/etc/default/keyboard).
    pub layout: String,
    pub variant: String,
    pub model: String,
    pub options: String,
    /// Milliseconds before a held key repeats.
    pub repeat_delay: i32,
    /// Repeats per second.
    pub repeat_rate: i32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Touchpad {
    /// Tap to click; libinput leaves it off.
    pub tap: bool,
    /// Content follows the fingers, as on macOS and phones. On by default.
    pub natural_scroll: bool,
    /// Pointer speed, -1.0 (slowest) to 1.0 (fastest); libinput's default
    /// is 0.
    pub speed: f64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            scale: None,
            keyboard: Keyboard::default(),
            touchpad: Touchpad::default(),
        }
    }
}

impl Default for Keyboard {
    fn default() -> Self {
        Self {
            layout: String::new(),
            variant: String::new(),
            model: String::new(),
            options: String::new(),
            repeat_delay: 200,
            repeat_rate: 25,
        }
    }
}

impl Default for Touchpad {
    fn default() -> Self {
        Self {
            tap: true,
            natural_scroll: true,
            speed: 0.0,
        }
    }
}

pub fn path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("patina-compositor").join("config.toml"))
}

/// The config at `path`, or what's wrong with it. A missing file is the
/// defaults. Warnings (unknown keys, values out of range) come with it.
pub fn load(path: &Path) -> Result<(Config, Vec<String>), String> {
    match std::fs::read_to_string(path) {
        Ok(text) => parse(&text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok((Config::default(), Vec::new())),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

pub fn parse(text: &str) -> Result<(Config, Vec<String>), String> {
    let table: toml::Table = toml::from_str(text).map_err(|e| e.to_string())?;
    let mut warnings = Vec::new();
    let mut config: Config = serde_ignored::deserialize(toml::Value::Table(table), |path| {
        warnings.push(format!("unknown key `{path}`, ignored"))
    })
    .map_err(|e| e.to_string())?;
    if let Some(scale) = config.scale
        && !(0.5..=4.0).contains(&scale)
    {
        warnings.push(format!(
            "scale {scale} is outside 0.5 to 4; using the automatic one"
        ));
        config.scale = None;
    }
    if !(-1.0..=1.0).contains(&config.touchpad.speed) {
        warnings.push(format!(
            "touchpad speed {} is outside -1 to 1; using 0",
            config.touchpad.speed
        ));
        config.touchpad.speed = 0.0;
    }
    if config.keyboard.repeat_rate < 0 || config.keyboard.repeat_delay < 0 {
        warnings.push("keyboard repeat values can't be negative; using the defaults".into());
        config.keyboard.repeat_rate = Keyboard::default().repeat_rate;
        config.keyboard.repeat_delay = Keyboard::default().repeat_delay;
    }
    Ok((config, warnings))
}

/// The scale to use: `PATINA_SCALE`, then the config, then `automatic`.
pub fn scale(config: &Config, automatic: f64) -> f64 {
    std::env::var("PATINA_SCALE")
        .ok()
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|s| (0.5..=4.0).contains(s))
        .or(config.scale)
        .unwrap_or(automatic)
}

/// The config file, or the defaults with what was wrong, for start-up.
pub fn load_or_default() -> Config {
    let Some(path) = path() else {
        return Config::default();
    };
    match load(&path) {
        Ok((config, warnings)) => {
            for w in warnings {
                eprintln!("patina-compositor: {}: {w}", path.display());
            }
            config
        }
        Err(e) => {
            eprintln!(
                "patina-compositor: {}: {e}; using the defaults",
                path.display()
            );
            Config::default()
        }
    }
}

impl crate::state::State {
    /// Reads config.toml again and applies it: Super+Shift+R. If the file
    /// has an error, the settings in use stay.
    pub fn reload_config(&mut self) {
        let Some(path) = path() else { return };
        match load(&path) {
            Ok((config, warnings)) => {
                for w in warnings {
                    eprintln!("patina-compositor: {}: {w}", path.display());
                }
                self.apply_config(config);
                eprintln!("patina-compositor: reloaded {}", path.display());
            }
            Err(e) => eprintln!("patina-compositor: {}: {e}; not reloaded", path.display()),
        }
    }

    fn apply_config(&mut self, config: Config) {
        let keyboard = self.seat.get_keyboard().expect("seat has a keyboard");
        let settings = crate::keyboard::KeyboardSettings::load(&config.keyboard);
        if let Err(e) = keyboard.set_xkb_config(self, settings.config()) {
            eprintln!(
                "patina-compositor: keyboard layout {:?}: {e:?}; keeping the current one",
                settings.layout
            );
        }
        keyboard.change_repeat_info(config.keyboard.repeat_rate, config.keyboard.repeat_delay);

        let new_scale = scale(&config, self.auto_scale);
        if new_scale != self.output.current_scale().fractional_scale() {
            self.set_scale(new_scale);
        }
        if let Some(u) = self.udev.as_mut() {
            u.configure_touchpads(&config.touchpad);
        }
        self.config = config;
        self.needs_redraw = true;
    }

    /// Changes the output's scale, and everything that follows from it.
    pub fn set_scale(&mut self, scale: f64) {
        self.output.change_current_state(
            None,
            None,
            Some(smithay::output::Scale::Fractional(scale)),
            None,
        );
        smithay::desktop::layer_map_for_output(&self.output).arrange();
        self.relayout_maximized();
        self.update_preferred_scale();
        if let Some(u) = self.udev.as_mut() {
            u.reload_cursor(scale);
        }
        self.needs_redraw = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_is_the_defaults() {
        let (c, w) = parse("").unwrap();
        assert_eq!(c, Config::default());
        assert!(w.is_empty());
        assert!(c.touchpad.tap);
        assert!(c.touchpad.natural_scroll);
        assert_eq!(c.keyboard.repeat_rate, 25);
    }

    #[test]
    fn reads_every_section() {
        let (c, w) = parse(
            "scale = 1.25\n[keyboard]\nlayout = \"us,se\"\nrepeat_rate = 40\n[touchpad]\nnatural_scroll = false\nspeed = 0.3\n",
        )
        .unwrap();
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(c.scale, Some(1.25));
        assert_eq!(c.keyboard.layout, "us,se");
        assert_eq!(c.keyboard.repeat_rate, 40);
        assert_eq!(c.keyboard.repeat_delay, 200);
        assert!(!c.touchpad.natural_scroll);
        assert!(c.touchpad.tap);
        assert_eq!(c.touchpad.speed, 0.3);
    }

    #[test]
    fn unknown_keys_and_bad_ranges_are_warnings() {
        let (c, w) = parse("scale = 9\ncolour = \"red\"\n[touchpad]\nspeed = 3.0\n").unwrap();
        assert_eq!(c.scale, None);
        assert_eq!(c.touchpad.speed, 0.0);
        assert_eq!(w.len(), 3, "{w:?}");
    }

    #[test]
    fn a_broken_file_is_an_error() {
        assert!(parse("scale = ").is_err());
        assert!(parse("scale = \"big\"").is_err());
    }
}
