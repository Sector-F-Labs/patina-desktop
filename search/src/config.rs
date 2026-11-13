//! The search config: whether the launcher searches files at all, and the
//! folders it looks in, each on or off and searched by name (fd), by what's
//! in the files (ripgrep) or both.
//!
//! ```toml
//! enabled = true
//!
//! [[folder]]
//! path = "~/Documents"
//! enabled = true
//! style = "both"   # "fd", "rg" or "both"
//! ```
//!
//! With no file, the defaults: Pictures, Music and Videos by name, Documents
//! and Notes by name and by contents.

use std::path::{Path, PathBuf};

use toml_edit::{ArrayOfTables, DocumentMut, Item, Table, value};

/// How a folder is searched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    /// By file name, with fd.
    Names,
    /// By what's in the files, with ripgrep.
    Contents,
    /// Both.
    Both,
}

impl Style {
    pub const ALL: [Style; 3] = [Style::Names, Style::Contents, Style::Both];

    /// The word the config file has for it.
    pub fn name(self) -> &'static str {
        match self {
            Style::Names => "fd",
            Style::Contents => "rg",
            Style::Both => "both",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Style::ALL.into_iter().find(|s| s.name() == name)
    }

    /// What Settings calls it.
    pub fn label(self) -> &'static str {
        match self {
            Style::Names => "File names (fd)",
            Style::Contents => "Contents (ripgrep)",
            Style::Both => "Names and contents (fd and ripgrep)",
        }
    }

    pub fn names(self) -> bool {
        matches!(self, Style::Names | Style::Both)
    }

    pub fn contents(self) -> bool {
        matches!(self, Style::Contents | Style::Both)
    }
}

/// A folder to search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folder {
    /// As written: a leading `~` is the home folder.
    pub path: String,
    pub enabled: bool,
    pub style: Style,
}

impl Folder {
    pub fn new(path: impl Into<String>, style: Style) -> Self {
        Self {
            path: path.into(),
            enabled: true,
            style,
        }
    }

    /// The folder on disk, `~` expanded.
    pub fn dir(&self) -> PathBuf {
        expand(&self.path)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Whether the launcher searches files at all.
    pub enabled: bool,
    pub folders: Vec<Folder>,
}

impl Default for Config {
    fn default() -> Self {
        let folder = |xdg: &str, fallback: &str, style| {
            Folder::new(
                short(&xdg_dir(xdg).unwrap_or_else(|| home().join(fallback))),
                style,
            )
        };
        Config {
            enabled: true,
            folders: vec![
                folder("PICTURES", "Pictures", Style::Names),
                folder("MUSIC", "Music", Style::Names),
                folder("VIDEOS", "Videos", Style::Names),
                folder("DOCUMENTS", "Documents", Style::Both),
                Folder::new("~/Notes", Style::Both),
            ],
        }
    }
}

impl Config {
    /// The folders that are on, and exist.
    pub fn active(&self) -> impl Iterator<Item = &Folder> {
        self.folders
            .iter()
            .filter(|f| f.enabled && f.dir().is_dir())
    }

    /// Reads `text`; what it doesn't say is the default (a folder with no
    /// style is searched by name), and a folder with no path is left out.
    pub fn parse(text: &str) -> Config {
        let Ok(doc) = text.parse::<DocumentMut>() else {
            return Config::default();
        };
        let default = Config::default();
        let folders = match doc.get("folder").and_then(Item::as_array_of_tables) {
            Some(tables) => tables
                .iter()
                .filter_map(|t| {
                    Some(Folder {
                        path: t.get("path")?.as_str()?.to_string(),
                        enabled: t.get("enabled").and_then(Item::as_bool).unwrap_or(true),
                        style: t
                            .get("style")
                            .and_then(Item::as_str)
                            .and_then(Style::from_name)
                            .unwrap_or(Style::Names),
                    })
                })
                .collect(),
            // No folders written: the file only turns search on or off.
            None => default.folders,
        };
        Config {
            enabled: doc
                .get("enabled")
                .and_then(Item::as_bool)
                .unwrap_or(default.enabled),
            folders,
        }
    }

    /// The config as the file has it.
    pub fn to_toml(&self) -> String {
        let mut doc = DocumentMut::new();
        doc["enabled"] = value(self.enabled);
        let mut folders = ArrayOfTables::new();
        for folder in &self.folders {
            let mut table = Table::new();
            table["path"] = value(folder.path.as_str());
            table["enabled"] = value(folder.enabled);
            table["style"] = value(folder.style.name());
            folders.push(table);
        }
        // An empty list is written as none, which reads back as the defaults:
        // a marker says it was emptied.
        doc["folder"] = Item::ArrayOfTables(folders);
        let mut text = doc.to_string();
        if self.folders.is_empty() {
            text.push_str("folders_cleared = true\n");
        }
        text
    }
}

/// The config file, `~/.config/patina-search/config.toml`.
pub fn path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home().join(".config"));
    Some(base.join("patina-search").join("config.toml"))
}

/// The config as it's saved, or the defaults.
pub fn read() -> Config {
    path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|t| parse_saved(&t))
        .unwrap_or_default()
}

/// [`Config::parse`], except that a file that was emptied of folders stays so.
fn parse_saved(text: &str) -> Config {
    let mut config = Config::parse(text);
    if text.contains("folders_cleared = true") {
        config.folders.clear();
    }
    config
}

/// Saves the config, making the folder if there's none.
pub fn write(config: &Config) -> Result<(), String> {
    let path = path().ok_or("no home folder")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(&path, config.to_toml()).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// `path` with a leading `~` made the home folder.
pub fn expand(path: &str) -> PathBuf {
    match path.strip_prefix('~') {
        Some("") => home(),
        Some(rest) if rest.starts_with('/') => home().join(rest.trim_start_matches('/')),
        _ => PathBuf::from(path),
    }
}

/// `path` as it's written in the config: `~` for the home folder.
pub fn short(path: &Path) -> String {
    match path.strip_prefix(home()) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".into(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

/// One of the user's folders by its XDG name (`PICTURES`), from
/// `~/.config/user-dirs.dirs`, where a language has its own words for them.
fn xdg_dir(name: &str) -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".config"));
    let text = std::fs::read_to_string(base.join("user-dirs.dirs")).ok()?;
    user_dir(&text, name, &home())
}

fn user_dir(text: &str, name: &str, home: &Path) -> Option<PathBuf> {
    let key = format!("XDG_{name}_DIR=");
    let value = text
        .lines()
        .map(str::trim)
        .find_map(|l| l.strip_prefix(key.as_str()))?
        .trim_matches('"');
    let dir = match value.strip_prefix("$HOME") {
        Some(rest) => home.join(rest.trim_start_matches('/')),
        None => PathBuf::from(value),
    };
    // A folder that's the home itself is XDG's "not set".
    (dir != home).then_some(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_search_media_by_name_and_documents_and_notes_both_ways() {
        let c = Config::default();
        assert!(c.enabled);
        let by = |name: &str| {
            c.folders
                .iter()
                .find(|f| f.path.ends_with(name))
                .map(|f| f.style)
        };
        assert_eq!(by("Pictures"), Some(Style::Names));
        assert_eq!(by("Music"), Some(Style::Names));
        assert_eq!(by("Videos"), Some(Style::Names));
        assert_eq!(by("Documents"), Some(Style::Both));
        assert_eq!(by("Notes"), Some(Style::Both));
    }

    #[test]
    fn the_file_is_read_back_as_written() {
        let config = Config {
            enabled: false,
            folders: vec![
                Folder::new("~/Pictures", Style::Names),
                Folder {
                    path: "/srv/notes".into(),
                    enabled: false,
                    style: Style::Contents,
                },
                Folder::new("~/a \"quoted\" one", Style::Both),
            ],
        };
        assert_eq!(parse_saved(&config.to_toml()), config);
    }

    #[test]
    fn what_the_file_leaves_out_is_the_default() {
        let c = Config::parse("enabled = false\n");
        assert!(!c.enabled);
        assert_eq!(c.folders, Config::default().folders);
        let c = Config::parse("[[folder]]\npath = \"~/x\"\n[[folder]]\nstyle = \"rg\"\n");
        assert_eq!(c.folders, vec![Folder::new("~/x", Style::Names)]);
        assert!(c.enabled);
        assert_eq!(Config::parse("not [ toml"), Config::default());
    }

    #[test]
    fn a_list_emptied_in_settings_stays_empty() {
        let none = Config {
            enabled: true,
            folders: vec![],
        };
        assert!(parse_saved(&none.to_toml()).folders.is_empty());
    }

    #[test]
    fn tilde_is_the_home_folder() {
        assert_eq!(expand("/srv/x"), PathBuf::from("/srv/x"));
        assert_eq!(expand("~"), home());
        assert_eq!(expand("~/a/b"), home().join("a/b"));
        assert_eq!(short(&home().join("a/b")), "~/a/b");
        assert_eq!(short(Path::new("/srv/x")), "/srv/x");
    }

    #[test]
    fn user_dirs_are_read_from_xdg() {
        let text = "XDG_PICTURES_DIR=\"$HOME/Bilder\"\nXDG_MUSIC_DIR=\"/data/music\"\nXDG_VIDEOS_DIR=\"$HOME/\"\n";
        let home = Path::new("/home/me");
        assert_eq!(
            user_dir(text, "PICTURES", home),
            Some("/home/me/Bilder".into())
        );
        assert_eq!(user_dir(text, "MUSIC", home), Some("/data/music".into()));
        assert_eq!(
            user_dir(text, "VIDEOS", home),
            None,
            "the home is 'not set'"
        );
        assert_eq!(user_dir(text, "DOCUMENTS", home), None);
    }
}
