//! The wallpapers on offer: images in the user's wallpapers folder, then
//! the system's backgrounds (Ubuntu's and GNOME's, or the ports' on the
//! BSDs).

use std::path::{Path, PathBuf};

/// Kinds patina-wallpaper decodes.
const EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp"];

/// The user's own wallpapers folder, `~/Pictures/Wallpapers`.
pub fn user_dir() -> Option<PathBuf> {
    dirs::picture_dir().map(|d| d.join("Wallpapers"))
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
fn images(dir: &Path) -> Vec<Wallpaper> {
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
