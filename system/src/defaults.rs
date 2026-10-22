//! Default apps (ADR-0079): which app opens each kind of link and file in
//! the Patina session. They're chosen in `~/.config/patina/defaults.toml`, a
//! role a line (`browser = "firefox"`), and [`sync`] writes them out as
//! `~/.config/patina-mimeapps.list`, which xdg-open and GLib read only when
//! `XDG_CURRENT_DESKTOP` is Patina, so other desktops keep their own. The
//! session's fallbacks are the installed `patina-mimeapps.list` (ADR-0066).

use std::io;
use std::path::PathBuf;

use toml_edit::{DocumentMut, value};

use crate::apps::Entry;

/// A kind of thing an app opens, as Settings' Defaults page offers it: a
/// browser for links, a viewer for images. Its first type decides which
/// apps are offered and which one is shown as the default; the app chosen
/// opens the rest of `types` too, where it takes them.
pub struct Role {
    pub name: &'static str,
    /// Its key in defaults.toml.
    pub key: &'static str,
    pub types: &'static [&'static str],
}

/// What the Defaults page offers: links, then the types
/// `patina-mimeapps.list` gives defaults for.
pub const ROLES: &[Role] = &[
    Role {
        name: "Web browser",
        key: "browser",
        types: &[
            "x-scheme-handler/https",
            "x-scheme-handler/http",
            "text/html",
            "application/xhtml+xml",
        ],
    },
    Role {
        name: "Email",
        key: "email",
        types: &["x-scheme-handler/mailto"],
    },
    Role {
        name: "Calendar",
        key: "calendar",
        types: &["text/calendar"],
    },
    Role {
        name: "Folders",
        key: "folders",
        types: &["inode/directory"],
    },
    Role {
        name: "Images",
        key: "images",
        types: &[
            "image/png",
            "image/jpeg",
            "image/gif",
            "image/webp",
            "image/avif",
            "image/heic",
            "image/jxl",
            "image/bmp",
            "image/tiff",
            "image/svg+xml",
            "image/x-portable-pixmap",
            "image/x-tga",
        ],
    },
    Role {
        name: "Documents",
        key: "documents",
        types: &["application/pdf", "application/epub+zip"],
    },
    Role {
        name: "Text",
        key: "text",
        types: &["text/plain"],
    },
    Role {
        name: "Music",
        key: "music",
        types: &[
            "audio/mpeg",
            "audio/flac",
            "audio/ogg",
            "audio/x-wav",
            "audio/mp4",
        ],
    },
    Role {
        name: "Video",
        key: "video",
        types: &[
            "video/mp4",
            "video/webm",
            "video/x-matroska",
            "video/quicktime",
        ],
    },
    Role {
        name: "Archives",
        key: "archives",
        types: &[
            "application/zip",
            "application/x-tar",
            "application/x-compressed-tar",
            "application/x-7z-compressed",
        ],
    },
];

/// The first line of the list [`sync`] writes, so it only ever replaces
/// its own.
const WRITTEN: &str = "# Written by Patina from ~/.config/patina/defaults.toml; edit that instead.";

impl Role {
    /// The apps that open this role's first type, by name.
    pub fn apps<'a>(&self, entries: &'a [Entry]) -> Vec<&'a Entry> {
        let mut apps: Vec<&Entry> = entries
            .iter()
            .filter(|e| e.mime_types.iter().any(|t| t == self.types[0]))
            .collect();
        apps.sort_by_key(|e| e.name.to_lowercase());
        apps
    }

    /// The id (`firefox`) of the app that opens this role's first type in
    /// the Patina session, if any is installed: the one chosen in
    /// defaults.toml, else what xdg-open would find.
    pub fn current(&self, entries: &[Entry]) -> Option<String> {
        let chosen = read().and_then(|doc| chosen(&doc, self.key));
        if let Some(id) = chosen.filter(|id| entries.iter().any(|e| e.id == *id)) {
            return Some(id);
        }
        let lists: Vec<(String, &str)> = lists()
            .into_iter()
            .filter_map(|(path, section)| Some((std::fs::read_to_string(path).ok()?, section)))
            .collect();
        let found = default_in(&lists, self.types[0], &|id| {
            entries.iter().any(|e| desktop_id(e) == id)
        })?;
        Some(found.trim_end_matches(".desktop").to_string())
    }

    /// Makes `app` this role's default in defaults.toml, keeping the rest of
    /// the file, and syncs the session's list.
    pub fn set(&self, app: &Entry, entries: &[Entry]) -> io::Result<()> {
        let path = path().ok_or_else(|| io::Error::other("there's no home folder"))?;
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let mut doc: DocumentMut = text.parse().map_err(io::Error::other)?;
        doc[self.key] = value(app.id.as_str());
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&path, doc.to_string())?;
        sync(entries)
    }
}

/// `entry`'s id as mimeapps.list names it.
pub fn desktop_id(entry: &Entry) -> String {
    format!("{}.desktop", entry.id)
}

/// `~/.config/patina/defaults.toml`, where the choices are kept.
pub fn path() -> Option<PathBuf> {
    Some(config_home()?.join("patina/defaults.toml"))
}

/// The list xdg-open reads in the Patina session.
fn list_path() -> Option<PathBuf> {
    Some(config_home()?.join("patina-mimeapps.list"))
}

fn read() -> Option<DocumentMut> {
    std::fs::read_to_string(path()?).ok()?.parse().ok()
}

/// The app id `doc` gives `key`, with or without `.desktop`.
fn chosen(doc: &DocumentMut, key: &str) -> Option<String> {
    let id = doc.get(key)?.as_str()?.trim();
    let id = id.strip_suffix(".desktop").unwrap_or(id);
    (!id.is_empty()).then(|| id.to_string())
}

/// Writes defaults.toml's choices as `~/.config/patina-mimeapps.list`, or
/// removes that list when there are none. A list of that name that Patina
/// didn't write is left alone, and said so. The compositor calls this at
/// start-up, so hand edits apply from the next session.
pub fn sync(entries: &[Entry]) -> io::Result<()> {
    let list = list_path().ok_or_else(|| io::Error::other("there's no home folder"))?;
    if let Ok(old) = std::fs::read_to_string(&list)
        && !old.starts_with(WRITTEN)
    {
        return Err(io::Error::other(format!(
            "{} isn't Patina's, so it's left as it is",
            list.display()
        )));
    }
    match read().map(|doc| mimeapps(&doc, entries)) {
        Some(text) if text.contains('=') => std::fs::write(&list, text),
        _ => match std::fs::remove_file(&list) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        },
    }
}

/// The mimeapps.list for `doc`'s choices: each role's app for its first
/// type, and for each of its other types the app says it takes.
fn mimeapps(doc: &DocumentMut, entries: &[Entry]) -> String {
    let mut text = format!("{WRITTEN}\n[Default Applications]\n");
    for role in ROLES {
        let Some(id) = chosen(doc, role.key) else {
            continue;
        };
        let app = entries.iter().find(|e| e.id == id);
        for (i, mime) in role.types.iter().enumerate() {
            if i == 0 || app.is_some_and(|a| a.mime_types.iter().any(|t| t == mime)) {
                text.push_str(&format!("{mime}={id}.desktop;\n"));
            }
        }
    }
    text
}

fn config_home() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .or_else(|| Some(PathBuf::from(std::env::var_os("HOME")?).join(".config")))
}

fn data_home() -> Option<PathBuf> {
    std::env::var_os("XDG_DATA_HOME")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .or_else(|| Some(PathBuf::from(std::env::var_os("HOME")?).join(".local/share")))
}

/// `var`'s folders, or `default`'s.
fn dirs_from(var: &str, default: &str) -> Vec<PathBuf> {
    let value = std::env::var(var).ok().filter(|v| !v.is_empty());
    std::env::split_paths(value.as_deref().unwrap_or(default)).collect()
}

/// The files a default is looked for in, first first, each with the
/// section that holds it, in the order xdg-mime's generic lookup reads them
/// in the Patina session: the config folders' lists, then the data
/// folders' `applications/` lists, and only then their `mimeinfo.cache`s,
/// the apps that merely take the type. `patina-mimeapps.list` comes before
/// the plain `mimeapps.list` beside it.
fn lists() -> Vec<(PathBuf, &'static str)> {
    let named = |dir: &PathBuf| {
        ["patina-mimeapps.list", "mimeapps.list"]
            .map(|name| (dir.join(name), "Default Applications"))
    };
    let mut lists = Vec::new();
    for dir in config_home()
        .into_iter()
        .chain(dirs_from("XDG_CONFIG_DIRS", "/etc/xdg"))
    {
        lists.extend(named(&dir));
    }
    let data: Vec<PathBuf> = data_home()
        .into_iter()
        .chain(dirs_from("XDG_DATA_DIRS", "/usr/local/share:/usr/share"))
        .map(|dir| dir.join("applications"))
        .collect();
    for dir in &data {
        lists.extend(named(dir));
        lists.push((dir.join("defaults.list"), "Default Applications"));
    }
    lists.extend(
        data.iter()
            .map(|dir| (dir.join("mimeinfo.cache"), "MIME Cache")),
    );
    lists
}

/// The ids `text` lists for `mime` in `[section]`.
fn listed<'a>(text: &'a str, section: &str, mime: &str) -> Vec<&'a str> {
    let mut inside = false;
    for line in text.lines().map(str::trim) {
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            inside = name == section;
        } else if inside
            && let Some((key, value)) = line.split_once('=')
            && key.trim() == mime
        {
            return value
                .split(';')
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .collect();
        }
    }
    Vec::new()
}

/// The first id that `is_installed` among those `lists` (each a file's
/// text and the section to look in, first first) give `mime`.
fn default_in(
    lists: &[(String, &str)],
    mime: &str,
    is_installed: &dyn Fn(&str) -> bool,
) -> Option<String> {
    lists
        .iter()
        .flat_map(|(text, section)| listed(text, section, mime))
        .find(|id| is_installed(id))
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, types: &[&str]) -> Entry {
        Entry {
            id: id.into(),
            name: id.into(),
            wm_class: None,
            icon: None,
            command: None,
            exec: None,
            terminal_emulator: false,
            mime_types: types.iter().map(|t| t.to_string()).collect(),
        }
    }

    #[test]
    fn the_first_installed_app_wins_in_list_order() {
        let mine =
            "[Default Applications]\nimage/png=gone.desktop;org.gnome.Loupe.desktop;\n".to_string();
        let session =
            "[Default Applications]\nimage/png=org.gnome.eog.desktop;\ntext/plain=gedit.desktop;\n"
                .to_string();
        let cache = "[MIME Cache]\ntext/plain=firefox.desktop;gedit.desktop;\n".to_string();
        let lists = [
            (mine, "Default Applications"),
            (session, "Default Applications"),
            (cache, "MIME Cache"),
        ];
        let installed = |id: &str| id != "gone.desktop";
        assert_eq!(
            default_in(&lists, "image/png", &installed).as_deref(),
            Some("org.gnome.Loupe.desktop")
        );
        assert_eq!(
            default_in(&lists, "text/plain", &installed).as_deref(),
            Some("gedit.desktop")
        );
        assert_eq!(default_in(&lists, "audio/flac", &installed), None);
    }

    #[test]
    fn other_sections_dont_count() {
        let text = "[Added Associations]\nimage/png=gimp.desktop;\n[Default Applications]\nimage/jpeg=loupe.desktop;\n";
        assert!(listed(text, "Default Applications", "image/png").is_empty());
        assert_eq!(
            listed(text, "Default Applications", "image/jpeg"),
            ["loupe.desktop"]
        );
    }

    #[test]
    fn a_choice_covers_the_types_its_app_takes() {
        let doc: DocumentMut =
            "browser = \"firefox\"\nimages = \"org.gnome.Loupe.desktop\"\ntext = \"\"\n"
                .parse()
                .unwrap();
        let entries = [
            entry(
                "firefox",
                &[
                    "x-scheme-handler/https",
                    "x-scheme-handler/http",
                    "text/html",
                ],
            ),
            entry("org.gnome.Loupe", &["image/png", "image/webp"]),
        ];
        assert_eq!(
            mimeapps(&doc, &entries),
            format!(
                "{WRITTEN}\n[Default Applications]\n\
                 x-scheme-handler/https=firefox.desktop;\n\
                 x-scheme-handler/http=firefox.desktop;\n\
                 text/html=firefox.desktop;\n\
                 image/png=org.gnome.Loupe.desktop;\n\
                 image/webp=org.gnome.Loupe.desktop;\n"
            )
        );
    }

    #[test]
    fn an_app_not_installed_keeps_its_first_type() {
        let doc: DocumentMut = "music = \"vlc\"\n".parse().unwrap();
        assert!(mimeapps(&doc, &[]).ends_with("[Default Applications]\naudio/mpeg=vlc.desktop;\n"));
    }

    #[test]
    fn keys_are_unique() {
        for (i, role) in ROLES.iter().enumerate() {
            assert!(
                !ROLES[..i].iter().any(|r| r.key == role.key),
                "{}",
                role.key
            );
        }
    }

    /// Every type the session gives a default for (ADR-0066) can be chosen
    /// on the Defaults page.
    #[test]
    fn every_session_default_is_offered() {
        let session = include_str!("../../packaging/patina-mimeapps.list");
        for line in session
            .lines()
            .filter(|l| l.contains('=') && !l.starts_with('#'))
        {
            let mime = line.split('=').next().unwrap();
            assert!(
                ROLES.iter().any(|r| r.types.contains(&mime)),
                "{mime} isn't on the Defaults page"
            );
        }
    }
}
