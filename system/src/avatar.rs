//! The user's picture (ADR-0104): `~/.face`, the file GNOME and KDE read
//! too, a square PNG. Settings writes it and the lock screen shows it. When
//! AccountsService is running it's told as well, so a login manager can show
//! the same picture.

use std::path::PathBuf;

use egui::ColorImage;
use image::imageops::FilterType;
use image::{DynamicImage, Rgba, RgbaImage};

/// The picture's side in pixels.
pub const SIZE: u32 = 256;

/// `~/.face`.
pub fn path() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(|h| PathBuf::from(h).join(".face"))
}

/// The picture, if there is one and it decodes.
pub fn load() -> Option<ColorImage> {
    // By what's inside, not the name: `.face` has no extension to go by.
    let image = image::load_from_memory(&std::fs::read(path()?).ok()?).ok()?;
    let small = square(image);
    Some(ColorImage::from_rgba_unmultiplied(
        [SIZE as usize; 2],
        small.as_raw(),
    ))
}

/// `image` cropped to a square from the middle, `SIZE` pixels a side.
pub fn square(image: DynamicImage) -> RgbaImage {
    let side = image.width().min(image.height()).max(1);
    let (x, y) = ((image.width() - side) / 2, (image.height() - side) / 2);
    image
        .crop_imm(x, y, side, side)
        .resize_exact(SIZE, SIZE, FilterType::Lanczos3)
        .to_rgba8()
}

/// The Sector F flask in `mark` on `background`, as a picture.
pub fn sector_f(background: [u8; 4], mark: [u8; 4]) -> RgbaImage {
    let mut picture = RgbaImage::from_pixel(SIZE, SIZE, Rgba(background));
    let side = SIZE * 3 / 5;
    let Some(flask) = crate::logo::rasterise(side) else {
        return picture;
    };
    let offset = (SIZE - side) / 2;
    for (i, pixel) in flask.pixels.iter().enumerate() {
        let (x, y) = (i as u32 % side + offset, i as u32 / side + offset);
        // The mask is white with only alpha meaning anything.
        let a = pixel.a() as u32;
        let under = picture.get_pixel_mut(x, y);
        for (below, above) in under.0.iter_mut().zip(mark).take(3) {
            *below = ((above as u32 * a + *below as u32 * (255 - a)) / 255) as u8;
        }
    }
    picture
}

/// Makes `picture` the user's, and tells AccountsService if it's there.
pub fn save(picture: &RgbaImage) -> Result<(), String> {
    let path = path().ok_or("no home folder")?;
    picture
        .save_with_format(&path, image::ImageFormat::Png)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    // Not having AccountsService, or not being allowed, isn't a failure:
    // the lock screen reads the file itself.
    let _ = tell_accounts(Some(&path));
    Ok(())
}

/// Takes the user's picture away.
pub fn remove() -> Result<(), String> {
    let path = path().ok_or("no home folder")?;
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("{}: {e}", path.display())),
    }
    let _ = tell_accounts(None);
    Ok(())
}

/// AccountsService's `SetIconFile`, for the user running this.
fn tell_accounts(path: Option<&std::path::Path>) -> Result<(), String> {
    use zbus::blocking::{Connection, Proxy};
    let bus = Connection::system().map_err(|e| e.to_string())?;
    let name = "org.freedesktop.Accounts";
    let manager =
        Proxy::new(&bus, name, "/org/freedesktop/Accounts", name).map_err(|e| e.to_string())?;
    // SAFETY: getuid has no preconditions.
    let uid = unsafe { libc_getuid() } as i64;
    let user: zbus::zvariant::OwnedObjectPath = manager
        .call("FindUserById", &(uid,))
        .map_err(|e| e.to_string())?;
    let user =
        Proxy::new(&bus, name, user, "org.freedesktop.Accounts.User").map_err(|e| e.to_string())?;
    let icon = path
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    user.call::<_, _, ()>("SetIconFile", &(icon,))
        .map_err(|e| e.to_string())
}

unsafe extern "C" {
    #[link_name = "getuid"]
    fn libc_getuid() -> u32;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pictures_are_cropped_square_from_the_middle() {
        let wide = DynamicImage::ImageRgba8(RgbaImage::from_fn(600, 200, |x, _| {
            if (200..400).contains(&x) {
                Rgba([255, 0, 0, 255])
            } else {
                Rgba([0, 0, 255, 255])
            }
        }));
        let square = square(wide);
        assert_eq!(square.dimensions(), (SIZE, SIZE));
        assert_eq!(square.get_pixel(0, 0).0, [255, 0, 0, 255]);
    }

    #[test]
    fn the_flask_is_in_the_middle_of_its_background() {
        let picture = sector_f([10, 20, 30, 255], [250, 250, 250, 255]);
        assert_eq!(picture.get_pixel(1, 1).0, [10, 20, 30, 255]);
        assert!(
            picture
                .pixels()
                .any(|p| p.0[0] > 200 && p.0[1] > 200 && p.0[2] > 200)
        );
    }
}

#[cfg(test)]
mod load_tests {
    use super::*;

    #[test]
    fn a_saved_picture_loads_back_from_a_file_with_no_extension() {
        let home = std::env::temp_dir().join(format!("patina-avatar-{}", std::process::id()));
        std::fs::create_dir_all(&home).unwrap();
        // SAFETY: no other test in this crate reads HOME.
        unsafe { std::env::set_var("HOME", &home) };
        save(&sector_f([10, 20, 30, 255], [250, 250, 250, 255])).unwrap();
        let loaded = load();
        let _ = std::fs::remove_dir_all(&home);
        assert!(loaded.is_some(), "~/.face was saved but won't load");
    }
}
