//! The window tabs' apps: their names (ADR-0061) and icons, from each
//! window's `.desktop` entry (`patina_system::apps`); the `Icon=` found
//! through the freedesktop icon theme and decoded at the size it's drawn.
//! This reads files, so it runs on a thread of its own (`spawn`).

use std::path::Path;
use std::sync::mpsc;

use egui::ColorImage;

use patina_system::apps::{Entry, entries, entry_for};

/// The icon `app_id`'s entry names (see `apps::entry_for`).
fn icon_name<'a>(entries: &'a [Entry], app_id: &str) -> Option<&'a str> {
    entry_for(entries, app_id)?.icon.as_deref()
}

/// `name` (an icon name, or a file path as some entries give) decoded to
/// `size` x `size` pixels.
fn load(name: &str, size: u32) -> Option<ColorImage> {
    let path = Path::new(name);
    let file = if path.is_absolute() {
        path.exists().then(|| path.to_path_buf())
    } else {
        // A size at least twice as big, scaled down: themes have 16, 24,
        // 32, 48..., and the nearest to an odd size (18) is often smaller,
        // which would be blurred scaling up.
        let source = (size * 2).max(32).min(u16::MAX as u32) as u16;
        freedesktop_icons::lookup(name)
            .with_size(source)
            // The theme chosen in Settings, as the launcher's and Files';
            // then hicolor and pixmaps.
            .with_theme(&patina_apps::icon::theme())
            .with_cache()
            .find()
    }?;
    let rgba = patina_apps::icon::render(&file, size)?;
    Some(ColorImage::from_rgba_premultiplied(
        [size as usize, size as usize],
        &rgba,
    ))
}

/// The icon for windows of `app_id`, if one can be found.
fn find(entries: &[Entry], app_id: &str, size: u32) -> Option<ColorImage> {
    match icon_name(entries, app_id) {
        Some(name) => load(name, size),
        // No entry: some apps install an icon named after their app id.
        None => load(app_id, size).or_else(|| load(&app_id.to_lowercase(), size)),
    }
}

/// The answer to `question`, from the apps installed now.
#[cfg(test)]
pub fn installed(question: Ask) -> Found {
    let known = entries();
    match question {
        Ask::Icon(app_id, size) => {
            let icon = find(&known, &app_id, size);
            Found::Icon(app_id, icon)
        }
        Ask::Name(app_id) => {
            let name = name(&known, &app_id);
            Found::Name(app_id, name)
        }
        Ask::Named(name, folder, size) => {
            let icon = named(&name, folder.as_deref(), size);
            Found::Named(named_key(&name, folder.as_deref()), icon)
        }
    }
}

/// What the bar asks about an app, by app id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ask {
    /// Its icon, this many pixels a side.
    Icon(String, u32),
    /// What it's called.
    Name(String),
    /// A tray item's icon by name, looked for in its own folder first
    /// (an item's IconThemePath), this many pixels a side (ADR-0064).
    Named(String, Option<String>, u32),
}

/// The answer: an app id's icon or name, if it has one.
#[derive(Debug, Clone, PartialEq)]
pub enum Found {
    Icon(String, Option<ColorImage>),
    Name(String, Option<String>),
    /// A tray icon, by `named_key`.
    Named(String, Option<ColorImage>),
}

/// The key a tray icon by name is found under.
pub fn named_key(name: &str, folder: Option<&str>) -> String {
    format!("{name}|{}", folder.unwrap_or(""))
}

/// The file for icon `name` in an app's own icon folder: `name.svg` or
/// `name.png` there, or in an icon theme's tree under it, the SVG or the
/// biggest PNG.
fn in_folder(folder: &Path, name: &str) -> Option<std::path::PathBuf> {
    fn walk(dir: &Path, name: &str, depth: u32, found: &mut Vec<std::path::PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && depth > 0 {
                walk(&path, name, depth - 1, found);
            } else if path.file_stem().is_some_and(|s| s == name)
                && path.extension().is_some_and(|e| e == "svg" || e == "png")
            {
                found.push(path);
            }
        }
    }
    let mut found = Vec::new();
    walk(folder, name, 4, &mut found);
    // An SVG, else the PNG in the biggest size's folder ("48x48").
    let size = |p: &std::path::PathBuf| {
        p.components()
            .filter_map(|c| {
                c.as_os_str()
                    .to_str()?
                    .split('x')
                    .next()?
                    .parse::<u32>()
                    .ok()
            })
            .max()
            .unwrap_or(0)
    };
    found
        .iter()
        .find(|p| p.extension().is_some_and(|e| e == "svg"))
        .cloned()
        .or_else(|| found.iter().max_by_key(|p| size(p)).cloned())
}

/// A tray icon by name, `size` pixels: from its folder, else the theme.
fn named(name: &str, folder: Option<&str>, size: u32) -> Option<ColorImage> {
    if let Some(file) = folder.and_then(|f| in_folder(Path::new(f), name)) {
        let rgba = patina_apps::icon::render(&file, size)?;
        return Some(ColorImage::from_rgba_premultiplied(
            [size as usize, size as usize],
            &rgba,
        ));
    }
    load(name, size)
}

/// The name of `app_id`'s app, from its entry.
fn name(entries: &[Entry], app_id: &str) -> Option<String> {
    entry_for(entries, app_id)
        .map(|e| e.name.clone())
        .filter(|n| !n.is_empty())
}

/// Answers each question asked on a thread, with `send`, which returns
/// false once nobody's listening. The entries are read once, and again
/// when an app id isn't among them, for apps installed since.
pub fn spawn(send: impl Fn(Found) -> bool + Send + 'static) -> mpsc::Sender<Ask> {
    let (ask, asked) = mpsc::channel::<Ask>();
    let started = std::thread::Builder::new()
        .name("icons".into())
        .spawn(move || {
            let mut known = entries();
            for question in asked {
                let app_id = match &question {
                    Ask::Icon(app_id, _) | Ask::Name(app_id) => Some(app_id),
                    Ask::Named(..) => None,
                };
                if app_id.is_some_and(|a| entry_for(&known, a).is_none()) {
                    known = entries();
                }
                let found = match question {
                    Ask::Named(name, folder, size) => {
                        let icon = named(&name, folder.as_deref(), size);
                        Found::Named(named_key(&name, folder.as_deref()), icon)
                    }
                    Ask::Icon(app_id, size) => {
                        let icon = find(&known, &app_id, size);
                        Found::Icon(app_id, icon)
                    }
                    Ask::Name(app_id) => {
                        let name = name(&known, &app_id);
                        Found::Name(app_id, name)
                    }
                };
                if !send(found) {
                    return;
                }
            }
        });
    if let Err(e) = started {
        eprintln!("patina-hud: no icons: {e}");
    }
    ask
}

/// What this machine finds for some app ids: `cargo test -p patina-hud
/// installed_icons -- --ignored --nocapture`.
#[cfg(test)]
#[test]
#[ignore = "reads the installed apps"]
fn installed_icons() {
    let known = entries();
    for app_id in [
        "firefox",
        "org.gnome.Nautilus",
        "foot",
        "teleprinter",
        "steam",
        "code",
        "com.sectorflabs.Settings",
    ] {
        let image = find(&known, app_id, 36);
        println!(
            "{app_id}: {:?} {:?}",
            icon_name(&known, app_id),
            image.map(|i| i.size)
        );
    }
}

/// What this machine finds for Steam's tray icon:
/// `cargo test -p patina-hud steams_tray_icon -- --ignored --nocapture`.
#[cfg(test)]
#[test]
#[ignore = "reads the installed Steam"]
fn steams_tray_icon() {
    let folder = dirs::home_dir().unwrap().join(".local/share/Steam/public");
    let file = in_folder(&folder, "steam_tray_mono");
    println!("{file:?}");
    let image = named("steam_tray_mono", folder.to_str(), 21);
    println!("{:?}", image.map(|i| i.size));
}
