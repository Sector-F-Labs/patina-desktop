//! Settings kept between runs in `~/.config/calendar/`: `config.toml` for
//! the app's settings, and `account.toml` (readable only by you) for the
//! server and login (ADR-0005).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::caldav::Account;
use crate::views::View;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub view: View,
    /// Calendars switched off in the sidebar, by href.
    pub hidden: Vec<String>,
    pub skin: Option<String>,
    pub system_title_bar: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self { view: View::Week, hidden: Vec::new(), skin: None, system_title_bar: false }
    }
}

fn dir() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("calendar"))
}

fn read<T: for<'de> Deserialize<'de> + Default>(file: &str) -> T {
    dir()
        .and_then(|d| std::fs::read_to_string(d.join(file)).ok())
        .and_then(|t| toml::from_str(&t).ok())
        .unwrap_or_default()
}

impl Config {
    pub fn load() -> Self {
        read("config.toml")
    }

    pub fn save(&self) {
        let Some(dir) = dir() else { return };
        let _ = std::fs::create_dir_all(&dir);
        if let Ok(text) = toml::to_string(self) {
            let _ = std::fs::write(dir.join("config.toml"), text);
        }
    }
}

pub fn load_account() -> Account {
    read("account.toml")
}

/// Saves the account where only you can read it.
pub fn save_account(account: &Account) -> Result<(), String> {
    let dir = dir().ok_or("No config folder")?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("account.toml");
    let text = toml::to_string(account).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&path)
            .map_err(|e| e.to_string())?;
        f.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        Ok(())
    }
    #[cfg(not(unix))]
    std::fs::write(&path, text).map_err(|e| e.to_string())
}
