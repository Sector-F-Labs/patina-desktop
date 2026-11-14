//! The notification daemon's settings (ADR-0141), in
//! `~/.config/patina-notify/config.toml`: whether notifications are shown, and
//! whether they wait while a fullscreen window has the keyboard. patina-notify
//! reads it each second; the tray's bell and Settings' page write it. Also
//! whether patina-notify is the daemon on the bus, so the bell isn't drawn
//! where it would do nothing.

use std::path::PathBuf;
use std::time::Duration;

use toml_edit::{DocumentMut, value};

const NAME: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";
/// What patina-notify calls itself in `GetServerInformation`.
const SERVER: &str = "patina-notify";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    pub enabled: bool,
    /// Hold notifications back while a fullscreen window has the keyboard, and
    /// show them when it's left.
    pub hide_in_fullscreen: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            enabled: true,
            hide_in_fullscreen: true,
        }
    }
}

/// The config file's path.
pub fn path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("patina-notify/config.toml"))
}

/// The settings from the file's text; the defaults for what's missing or wrong.
pub fn parse(text: &str) -> Config {
    let doc = text.parse::<DocumentMut>().ok();
    let flag = |key: &str| doc.as_ref().and_then(|d| d.get(key)?.as_bool());
    Config {
        enabled: flag("enabled").unwrap_or(true),
        hide_in_fullscreen: flag("hide_in_fullscreen").unwrap_or(true),
    }
}

/// The settings now; the defaults when there's no file.
pub fn read() -> Config {
    path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|t| parse(&t))
        .unwrap_or_default()
}

/// `text` with `config` written into it, whatever else it holds kept.
pub fn render(text: &str, config: Config) -> String {
    let mut doc = text.parse::<DocumentMut>().unwrap_or_default();
    doc["enabled"] = value(config.enabled);
    doc["hide_in_fullscreen"] = value(config.hide_in_fullscreen);
    doc.to_string()
}

/// Saves `config`, keeping the rest of the file.
pub fn write(config: Config) -> Result<(), String> {
    let path = path().ok_or("no config folder")?;
    let old = std::fs::read_to_string(&path).unwrap_or_default();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, render(&old, config)).map_err(|e| e.to_string())
}

/// Whether patina-notify has the notifications' name on the session bus.
fn served(bus: &zbus::blocking::Connection) -> bool {
    bus.call_method(Some(NAME), PATH, Some(NAME), "GetServerInformation", &())
        .ok()
        .and_then(|reply| {
            reply
                .body()
                .deserialize::<(String, String, String, String)>()
                .ok()
        })
        .is_some_and(|(name, ..)| name == SERVER)
}

/// Calls `f` now and every two seconds with the settings while patina-notify
/// is the daemon (`None` while it isn't: nested, or another desktop's), until
/// it returns false. For the tray's bell.
pub fn watch(mut f: impl FnMut(Option<Config>) -> bool) {
    let Ok(bus) = zbus::blocking::Connection::session() else {
        f(None);
        return;
    };
    loop {
        if !f(served(&bus).then(read)) {
            return;
        }
        std::thread::sleep(Duration::from_secs(2));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn on_unless_the_file_says_otherwise() {
        assert!(parse("").enabled);
        assert!(parse("enabled = true").enabled);
        assert!(!parse("enabled = false").enabled);
        assert!(parse("enabled = \"no\"").enabled);
        assert!(parse("not toml at all [").enabled);
    }

    #[test]
    fn held_back_in_fullscreen_unless_the_file_says_otherwise() {
        assert!(parse("").hide_in_fullscreen);
        assert!(!parse("hide_in_fullscreen = false").hide_in_fullscreen);
        assert!(parse("hide_in_fullscreen = 3").hide_in_fullscreen);
        assert!(!parse("enabled = false\nhide_in_fullscreen = false").hide_in_fullscreen);
    }

    #[test]
    fn what_is_written_is_read_back_and_the_rest_is_kept() {
        let config = Config {
            enabled: false,
            hide_in_fullscreen: false,
        };
        let text = render("# mine\nextra = 1\n", config);
        assert_eq!(parse(&text), config);
        assert!(text.contains("# mine") && text.contains("extra = 1"));
        assert_eq!(parse(&render("", Config::default())), Config::default());
    }
}
