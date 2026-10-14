//! App icons: found through the freedesktop icon theme spec, decoded from
//! SVG or PNG to the size they're drawn at. This reads files, so it runs on
//! a thread of its own (`spawn`), never on the UI thread.

use std::path::{Path, PathBuf};

use egui::ColorImage;

/// The theme searched first. A lookup that misses it falls back to
/// hicolor, where apps install their own icons, then to the pixmaps folder.
const THEME: &str = "Adwaita";

/// Finds `name` (an icon name, or a file path as some entries give) as a
/// file, near `size` pixels.
pub fn find(name: &str, size: u16) -> Option<PathBuf> {
    let path = Path::new(name);
    if path.is_absolute() {
        return path.exists().then(|| path.to_path_buf());
    }
    freedesktop_icons::lookup(name)
        .with_size(size)
        .with_theme(THEME)
        .with_cache()
        .find()
}

/// Decodes the icon at `path` to `size` x `size` pixels, keeping its aspect
/// ratio, centred.
pub fn decode(path: &Path, size: u32) -> Option<ColorImage> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    let rgba = match ext.as_str() {
        "svg" | "svgz" => svg(path, size)?,
        "png" => png(path, size)?,
        _ => return None,
    };
    Some(ColorImage::from_rgba_premultiplied(
        [size as usize, size as usize],
        &rgba,
    ))
}

/// Premultiplied RGBA, as tiny-skia renders.
fn svg(path: &Path, size: u32) -> Option<Vec<u8>> {
    let data = std::fs::read(path).ok()?;
    let tree = resvg::usvg::Tree::from_data(&data, &resvg::usvg::Options::default()).ok()?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(size, size)?;
    let (w, h) = (tree.size().width(), tree.size().height());
    let scale = size as f32 / w.max(h);
    let dx = (size as f32 - w * scale) / 2.0;
    let dy = (size as f32 - h * scale) / 2.0;
    let transform = resvg::tiny_skia::Transform::from_scale(scale, scale).post_translate(dx, dy);
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    Some(pixmap.take())
}

/// Premultiplied RGBA, resized to fit.
fn png(path: &Path, size: u32) -> Option<Vec<u8>> {
    let image = image::open(path).ok()?.into_rgba8();
    let fitted = image::imageops::resize(&image, size, size, image::imageops::FilterType::Triangle);
    Some(
        fitted
            .pixels()
            .flat_map(|p| {
                let [r, g, b, a] = p.0;
                let m = |c: u8| ((c as u16 * a as u16 + 127) / 255) as u8;
                [m(r), m(g), m(b), a]
            })
            .collect(),
    )
}

/// Looks up and decodes each `(index, icon name)` on a thread, sending each
/// image as it's ready, so the list shows straight away and icons fill in.
pub fn spawn(
    wanted: Vec<(usize, String)>,
    size: u32,
    send: impl Fn(usize, ColorImage) -> bool + Send + 'static,
) {
    std::thread::Builder::new()
        .name("icons".into())
        .spawn(move || {
            for (index, name) in wanted {
                let image = find(&name, size as u16).and_then(|p| decode(&p, size));
                if let Some(image) = image
                    && !send(index, image)
                {
                    return;
                }
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_svg_and_png_to_the_size_asked() {
        let dir =
            std::env::temp_dir().join(format!("patina-launcher-icons-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let svg = dir.join("wide.svg");
        std::fs::write(
            &svg,
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><rect width="20" height="10" fill="red"/></svg>"#,
        )
        .unwrap();
        let image = decode(&svg, 32).expect("svg decodes");
        assert_eq!(image.size, [32, 32]);
        // Wide, so centred with transparent bands above and below.
        assert_eq!(image.pixels[0].a(), 0);
        assert_eq!(image.pixels[16 * 32 + 16], egui::Color32::RED);

        let png = dir.join("dot.png");
        image::RgbaImage::from_pixel(4, 4, image::Rgba([0, 0, 128, 128]))
            .save(&png)
            .unwrap();
        let image = decode(&png, 8).expect("png decodes");
        assert_eq!(image.size, [8, 8]);
        assert_eq!(image.pixels[0].a(), 128);
        assert_eq!(image.pixels[0].b(), 64, "premultiplied");

        assert!(decode(&dir.join("x.xpm"), 8).is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn absolute_paths_are_used_as_they_are() {
        assert_eq!(find("/no/such/icon.png", 32), None);
    }
}
