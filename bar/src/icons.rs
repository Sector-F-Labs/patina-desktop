//! The window tabs' app icons: a window's app id (a Wayland app id, or an
//! X11 window's class) matched to an installed `.desktop` entry, its `Icon=`
//! found through the freedesktop icon theme, and decoded at the size it's
//! drawn. This reads files, so it runs on a thread of its own (`spawn`).

use std::path::Path;
use std::sync::mpsc;

use egui::ColorImage;
use freedesktop_desktop_entry::{Iter, default_paths};

/// The theme searched first, as the launcher's; then hicolor and pixmaps.
const THEME: &str = "Adwaita";

/// What a `.desktop` entry says that matters here.
struct Entry {
    id: String,
    wm_class: Option<String>,
    icon: String,
}

fn entries() -> Vec<Entry> {
    let mut seen = std::collections::HashSet::new();
    Iter::new(default_paths())
        .entries::<&str>(None)
        .filter(|e| seen.insert(e.id().to_string()))
        .filter_map(|e| {
            Some(Entry {
                id: e.id().to_string(),
                wm_class: e.startup_wm_class().map(str::to_string),
                icon: e.icon()?.to_string(),
            })
        })
        .collect()
}

/// The icon `app_id`'s entry names: the entry with that id, or whose
/// StartupWMClass is it, or whose id ends in it (`nautilus` for
/// org.gnome.Nautilus), ignoring case.
fn icon_name<'a>(entries: &'a [Entry], app_id: &str) -> Option<&'a str> {
    let last = |id: &str| id.rsplit('.').next().unwrap_or(id).to_lowercase();
    let wanted = app_id.to_lowercase();
    let by = |matches: &dyn Fn(&Entry) -> bool| entries.iter().find(|e| matches(e));
    by(&|e| e.id.eq_ignore_ascii_case(app_id))
        .or_else(|| {
            by(&|e| {
                e.wm_class
                    .as_deref()
                    .is_some_and(|c| c.eq_ignore_ascii_case(app_id))
            })
        })
        .or_else(|| by(&|e| last(&e.id) == wanted))
        .map(|e| e.icon.as_str())
}

/// `name` (an icon name, or a file path as some entries give) decoded to
/// `size` x `size` pixels.
fn load(name: &str, size: u32) -> Option<ColorImage> {
    let path = Path::new(name);
    let file = if path.is_absolute() {
        path.exists().then(|| path.to_path_buf())
    } else {
        freedesktop_icons::lookup(name)
            .with_size(size.min(u16::MAX as u32) as u16)
            .with_theme(THEME)
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

/// The icon for windows of `app_id`, from the apps installed now.
#[cfg(test)]
pub fn find_installed(app_id: &str, size: u32) -> Option<ColorImage> {
    find(&entries(), app_id, size)
}

/// Answers each `(app id, size)` asked for on a thread, with `send`, which
/// returns false once nobody's listening. The entries are read once, and
/// again when an app id isn't among them, for apps installed since.
pub fn spawn(
    send: impl Fn(String, Option<ColorImage>) -> bool + Send + 'static,
) -> mpsc::Sender<(String, u32)> {
    let (ask, asked) = mpsc::channel::<(String, u32)>();
    let started = std::thread::Builder::new()
        .name("icons".into())
        .spawn(move || {
            let mut known = entries();
            for (app_id, size) in asked {
                if icon_name(&known, &app_id).is_none() {
                    known = entries();
                }
                if !send(app_id.clone(), find(&known, &app_id, size)) {
                    return;
                }
            }
        });
    if let Err(e) = started {
        eprintln!("patina-bar: no icons: {e}");
    }
    ask
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, wm_class: Option<&str>, icon: &str) -> Entry {
        Entry {
            id: id.into(),
            wm_class: wm_class.map(Into::into),
            icon: icon.into(),
        }
    }

    #[test]
    fn app_ids_find_their_entrys_icon() {
        let entries = [
            entry("org.gnome.Nautilus", None, "org.gnome.Nautilus"),
            entry("firefox", None, "firefox"),
            entry("steam", Some("Steam"), "steam"),
            entry("com.mitchellh.ghostty", None, "ghostty-icon"),
        ];
        assert_eq!(icon_name(&entries, "firefox"), Some("firefox"));
        assert_eq!(
            icon_name(&entries, "org.gnome.nautilus"),
            Some("org.gnome.Nautilus")
        );
        assert_eq!(
            icon_name(&entries, "Steam"),
            Some("steam"),
            "by its X11 class"
        );
        assert_eq!(
            icon_name(&entries, "ghostty"),
            Some("ghostty-icon"),
            "by the id's end"
        );
        assert_eq!(icon_name(&entries, "foot"), None);
    }
}

/// What this machine finds for some app ids: `cargo test -p patina-bar
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
    ] {
        let image = find(&known, app_id, 36);
        println!(
            "{app_id}: {:?} {:?}",
            icon_name(&known, app_id),
            image.map(|i| i.size)
        );
    }
}
