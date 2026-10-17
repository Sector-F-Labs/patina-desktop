//! The window tabs' app icons: the `Icon=` of a window's `.desktop` entry
//! (`apps.rs`) found through the freedesktop icon theme, and decoded at the
//! size it's drawn. This reads files, so it runs on a thread of its own (`spawn`).

use std::path::Path;
use std::sync::mpsc;

use egui::ColorImage;

use crate::apps::{Entry, entries, entry_for};

/// The theme searched first, as the launcher's; then hicolor and pixmaps.
const THEME: &str = "Adwaita";

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
