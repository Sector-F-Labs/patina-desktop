//! Settings kept between runs in `~/.config/ledger/config.toml`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const RECENT: usize = 8;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub skin: Option<String>,
    pub system_title_bar: bool,
    /// Most recent first; the first is reopened at start-up.
    pub recent: Vec<PathBuf>,
    /// Ledgers whose shell formulas may run (ADR-0006). Kept here, never in the
    /// file, so a file can't vouch for itself.
    pub trusted: Vec<PathBuf>,
}

fn file() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("ledger").join("config.toml"))
}

impl Config {
    pub fn load() -> Self {
        file().and_then(|f| std::fs::read_to_string(f).ok()).and_then(|t| toml::from_str(&t).ok()).unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(file) = file() else { return };
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = toml::to_string(self) {
            let _ = std::fs::write(file, text);
        }
    }

    /// Puts a file first in the recent list.
    pub fn used(&mut self, path: &Path) {
        self.recent.retain(|p| p != path);
        self.recent.insert(0, path.to_path_buf());
        self.recent.truncate(RECENT);
        self.save();
    }

    pub fn is_trusted(&self, path: &Path) -> bool {
        let path = canonical(path);
        self.trusted.contains(&path)
    }

    pub fn set_trusted(&mut self, path: &Path, trusted: bool) {
        let path = canonical(path);
        self.trusted.retain(|p| *p != path);
        if trusted {
            self.trusted.push(path);
        }
        self.save();
    }

    /// Forgets a file that couldn't be opened.
    pub fn forget(&mut self, path: &Path) {
        self.recent.retain(|p| p != path);
        self.save();
    }
}

/// The file's real path, so a link or `..` can't make one file two.
fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_is_unique_and_bounded() {
        let mut c = Config::default();
        for i in 0..12 {
            c.recent.insert(0, PathBuf::from(format!("/f{i}")));
        }
        c.recent.retain(|p| p != Path::new("/f3"));
        c.recent.insert(0, "/f3".into());
        c.recent.truncate(RECENT);
        assert_eq!(c.recent[0], PathBuf::from("/f3"));
        assert_eq!(c.recent.len(), RECENT);
    }
}
