//! Opening and saving pictures. The work runs on a thread of its own, file
//! dialogs included, so the window never waits on the disk; the app picks
//! up the [`Done`] it sends.

use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

use crate::bitmap::Bitmap;

pub enum Done {
    Opened(PathBuf, Bitmap),
    Saved(PathBuf),
    Failed(String),
    /// A dialog was closed without choosing a file.
    Cancelled,
}

const EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "bmp", "gif", "webp"];

/// Opens `path`, or asks for a file when it's `None`.
pub fn open(ctx: &egui::Context, tx: Sender<Done>, path: Option<PathBuf>) {
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let path = path
            .or_else(|| rfd::FileDialog::new().set_title("Open Image").add_filter("Images", EXTENSIONS).pick_file());
        let done = match path {
            Some(path) => match read(&path) {
                Ok(bitmap) => Done::Opened(path, bitmap),
                Err(e) => Done::Failed(format!("Couldn't open {}: {e}", name(&path))),
            },
            None => Done::Cancelled,
        };
        let _ = tx.send(done);
        ctx.request_repaint();
    });
}

/// Saves `bitmap` to `path`, or asks where when it's `None`.
pub fn save(ctx: &egui::Context, tx: Sender<Done>, bitmap: Bitmap, path: Option<PathBuf>, suggest: &str) {
    let ctx = ctx.clone();
    let suggest = suggest.to_string();
    std::thread::spawn(move || {
        let path = path.or_else(|| {
            rfd::FileDialog::new()
                .set_title("Save Image")
                .set_file_name(suggest)
                .add_filter("PNG", &["png"])
                .add_filter("JPEG", &["jpg", "jpeg"])
                .add_filter("BMP", &["bmp"])
                .save_file()
        });
        let done = match path {
            Some(path) => match write(&path, &bitmap) {
                Ok(()) => Done::Saved(path),
                Err(e) => Done::Failed(format!("Couldn't save {}: {e}", name(&path))),
            },
            None => Done::Cancelled,
        };
        let _ = tx.send(done);
        ctx.request_repaint();
    });
}

pub fn name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string())
}

/// Reads an image, flattened onto white where it's transparent.
pub fn read(path: &Path) -> Result<Bitmap, String> {
    let image = image::open(path).map_err(|e| e.to_string())?.to_rgba8();
    let (w, h) = image.dimensions();
    let mut bitmap = Bitmap::from_rgba(w as usize, h as usize, image.as_raw());
    for p in &mut bitmap.pixels {
        let a = p[3] as u32;
        for c in &mut p[..3] {
            *c = ((*c as u32 * a + 255 * (255 - a)) / 255) as u8;
        }
        p[3] = 255;
    }
    Ok(bitmap)
}

/// Writes the format the extension names; PNG when there is none.
pub fn write(path: &Path, bitmap: &Bitmap) -> Result<(), String> {
    let (w, h) = (bitmap.width as u32, bitmap.height as u32);
    let rgba = image::RgbaImage::from_raw(w, h, bitmap.to_bytes()).ok_or("the picture has the wrong size")?;
    let is_jpeg = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("jpg") || e.eq_ignore_ascii_case("jpeg"));
    let result = if is_jpeg {
        image::DynamicImage::ImageRgba8(rgba).to_rgb8().save(path)
    } else if path.extension().is_none() {
        rgba.save_with_format(path.with_extension("png"), image::ImageFormat::Png)
    } else {
        rgba.save(path)
    };
    result.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitmap::WHITE;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("draw-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    #[test]
    fn png_round_trips() {
        let mut b = Bitmap::new(3, 2, WHITE);
        b.put(2, 1, [10, 20, 30, 255]);
        let path = scratch("a.png");
        write(&path, &b).unwrap();
        assert_eq!(read(&path).unwrap(), b);
    }

    #[test]
    fn jpeg_and_bmp_save_and_read_back_the_same_size() {
        let b = Bitmap::new(8, 4, [200, 30, 30, 255]);
        for file in ["a.jpg", "a.bmp"] {
            let path = scratch(file);
            write(&path, &b).unwrap();
            let back = read(&path).unwrap();
            assert_eq!((back.width, back.height), (8, 4));
        }
    }

    #[test]
    fn transparency_is_flattened_onto_white() {
        let path = scratch("clear.png");
        image::RgbaImage::from_pixel(1, 1, image::Rgba([0, 0, 0, 0])).save(&path).unwrap();
        assert_eq!(read(&path).unwrap().get(0, 0), Some(WHITE));
    }

    #[test]
    fn a_missing_file_is_an_error() {
        assert!(read(&scratch("nope.png")).is_err());
    }
}
