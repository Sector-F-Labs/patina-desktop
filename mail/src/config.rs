//! Settings kept between runs in `~/.config/mail/`: `config.toml` for the
//! app's settings, and `account.toml` (readable only by you) for the Bridge
//! login (ADR-0003).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::message::Order;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// The folder last shown.
    pub folder: String,
    pub skin: Option<String>,
    /// "light" or "dark" while the skin follows the desktop; anything else follows it.
    pub light_dark: String,
    pub system_title_bar: bool,
    /// Show HTML mail as converted text instead of drawing it (ADR-0005).
    pub show_text: bool,
    /// How the message list is sorted, for every folder.
    pub order: Order,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            folder: "INBOX".into(),
            skin: None,
            light_dark: "system".into(),
            system_title_bar: false,
            show_text: false,
            order: Order::default(),
        }
    }
}

/// How a connection is secured. Bridge offers STARTTLS by default, and SSL
/// if switched to it in its settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Security {
    #[default]
    StartTls,
    Ssl,
}

/// The Proton Mail Bridge login, from the Bridge app's mailbox details.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Account {
    pub host: String,
    pub imap_port: u16,
    pub smtp_port: u16,
    pub security: Security,
    /// Bridge's username: the account's email address.
    pub username: String,
    /// Bridge's own generated password, not the Proton account's.
    pub password: String,
    /// The name sent with your address.
    pub name: String,
}

impl Default for Account {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            imap_port: 1143,
            smtp_port: 1025,
            security: Security::StartTls,
            username: String::new(),
            password: String::new(),
            name: String::new(),
        }
    }
}

impl Account {
    pub fn is_set(&self) -> bool {
        !self.host.is_empty() && !self.username.is_empty() && !self.password.is_empty()
    }

    /// Bridge's certificate is self-signed, so it's only trusted unchecked
    /// on this machine, where Bridge runs (ADR-0002).
    pub fn is_local(&self) -> bool {
        matches!(self.host.as_str(), "127.0.0.1" | "localhost" | "::1")
    }
}

fn dir() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("mail"))
}

fn read<T: for<'de> Deserialize<'de> + Default>(file: &str) -> T {
    dir()
        .and_then(|d| std::fs::read_to_string(d.join(file)).ok())
        .and_then(|t| toml::from_str(&t).ok())
        .unwrap_or_default()
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
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&path)
            .map_err(|e| e.to_string())?;
        f.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        Ok(())
    }
    #[cfg(not(unix))]
    std::fs::write(&path, text).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_defaults_to_bridge() {
        let a: Account = toml::from_str("username = 'me@proton.me'\npassword = 'x'").unwrap();
        assert_eq!((a.host.as_str(), a.imap_port, a.smtp_port), ("127.0.0.1", 1143, 1025));
        assert_eq!(a.security, Security::StartTls);
        assert!(a.is_set() && a.is_local());
    }

    #[test]
    fn security_round_trips() {
        let a = Account { security: Security::Ssl, ..Default::default() };
        let text = toml::to_string(&a).unwrap();
        assert!(text.contains("security = \"ssl\""));
        assert_eq!(toml::from_str::<Account>(&text).unwrap().security, Security::Ssl);
    }
}
