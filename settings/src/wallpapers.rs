//! The wallpapers on offer: images in the user's wallpapers folder, then
//! the system's backgrounds (Ubuntu's and GNOME's, or the ports' on the
//! BSDs), and small pictures of them for the Wallpaper page.

use std::path::{Path, PathBuf};
use std::sync::mpsc;

use patina_system::wallpaper::Fit;

/// Kinds patina-wallpaper decodes.
const EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp"];

/// The user's own wallpapers folder, `~/Pictures/Wallpapers`.
pub fn user_dir() -> Option<PathBuf> {
    dirs::picture_dir().map(|d| d.join("Wallpapers"))
}

/// `wallpaper_fit` in the bar's config.
pub fn configured_fit() -> Fit {
    let text = patina_shell::bar::config_path().and_then(|p| std::fs::read_to_string(p).ok());
    let name = text.and_then(|t| patina_shell::bar::top_level(&t, "wallpaper_fit"));
    Fit::from_name(name.as_deref())
}

/// A wallpaper: its name to show and its file.
pub struct Wallpaper {
    pub name: String,
    pub path: PathBuf,
}

/// The images in the user's folder, then the system's, each by name.
pub fn find() -> Vec<Wallpaper> {
    let system = ["/usr/share/backgrounds", "/usr/local/share/backgrounds"].map(PathBuf::from);
    user_dir()
        .into_iter()
        .chain(system)
        .flat_map(|dir| images(&dir))
        .collect()
}

/// The images directly in `dir`, by name.
pub fn images(dir: &Path) -> Vec<Wallpaper> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<_> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file() && is_image(p))
        .map(|path| Wallpaper {
            name: name(&path),
            path,
        })
        .collect();
    found.sort_by_key(|w| w.name.to_lowercase());
    found
}

fn is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

/// The file's name without its extension, with underscores as spaces.
fn name(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().replace('_', " "))
        .unwrap_or_default()
}

/// The thumbnails' longest side, in pixels.
const THUMBNAIL: u32 = 320;

/// Decodes a thumbnail of each of `paths` on a thread of its own, sending
/// each as it's done (`None` for one that won't decode) and asking `ctx` to
/// repaint.
pub fn thumbnails(
    paths: Vec<PathBuf>,
    ctx: egui::Context,
) -> mpsc::Receiver<(PathBuf, Option<egui::ColorImage>)> {
    let (send, receive) = mpsc::channel();
    let _ = std::thread::Builder::new()
        .name("thumbnails".into())
        .spawn(move || {
            for path in paths {
                let image = image::open(&path).ok().map(|i| {
                    let small = i.thumbnail(THUMBNAIL, THUMBNAIL).to_rgba8();
                    egui::ColorImage::from_rgba_unmultiplied(
                        [small.width() as usize, small.height() as usize],
                        small.as_raw(),
                    )
                });
                if send.send((path, image)).is_err() {
                    return;
                }
                ctx.request_repaint();
            }
        });
    receive
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn images_by_extension_and_names_without_it() {
        assert!(is_image(Path::new("/a/Red_Acer.jpg")));
        assert!(is_image(Path::new("/a/b.WEBP")));
        assert!(!is_image(Path::new("/a/b.CR3")));
        assert!(!is_image(Path::new("/a/contest")));
        assert_eq!(
            name(Path::new("/a/mendhak-Red_Acer.jpg")),
            "mendhak-Red Acer"
        );
    }
}
