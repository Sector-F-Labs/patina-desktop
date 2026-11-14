//! Album art for the cards (ADR-0144): a player's cover file, read off the UI thread and
//! shrunk to a square thumbnail. A cover that can't be read is dropped from its player, so
//! its card keeps the look it has without art and the menu is never sized for a picture
//! that won't come.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use patina_system::mpris::Player;

/// The thumbnail's side in pixels: twice the 64 it is drawn at, for a screen at scale 2.
const SIDE: u32 = 128;

/// A cover, square, as RGBA.
pub struct Thumb {
    pub size: [usize; 2],
    pub rgba: Vec<u8>,
}

/// Read and shrink the picture at `path`, filling the square (the middle of a wide one).
pub fn load(path: &Path) -> Option<Thumb> {
    let image = image::open(path).ok()?;
    let image = image.resize_to_fill(SIDE, SIDE, image::imageops::FilterType::Triangle);
    let rgba = image.to_rgba8();
    Some(Thumb {
        size: [rgba.width() as usize, rgba.height() as usize],
        rgba: rgba.into_raw(),
    })
}

/// What has been read, so a cover is decoded once and not at every read of the players.
#[derive(Default)]
pub struct Covers {
    /// By path, with the file's time, so a cover written over under the same name is read again.
    seen: HashMap<PathBuf, (Option<SystemTime>, Option<Arc<Thumb>>)>,
}

impl Covers {
    /// Check each player's cover: keep it when it can be read, drop it when it can't. Returns
    /// the thumbnails read this time, for the menu to draw.
    pub fn resolve(&mut self, players: &mut [Player]) -> Vec<(PathBuf, Arc<Thumb>)> {
        let mut fresh = Vec::new();
        for player in players {
            let Some(path) = player.art.clone() else {
                continue;
            };
            let time = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
            let known = self.seen.get(&path).is_some_and(|(t, _)| *t == time);
            if !known {
                let thumb = load(&path).map(Arc::new);
                if let Some(thumb) = &thumb {
                    fresh.push((path.clone(), thumb.clone()));
                }
                self.seen.insert(path.clone(), (time, thumb));
            }
            if self.seen.get(&path).is_none_or(|(_, t)| t.is_none()) {
                player.art = None;
            }
        }
        fresh
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patina_system::mpris::Status;

    fn player(art: Option<PathBuf>) -> Player {
        Player {
            bus_name: "org.mpris.MediaPlayer2.t".into(),
            identity: "T".into(),
            status: Status::Playing,
            title: "Song".into(),
            artist: String::new(),
            art,
            can_previous: true,
            can_next: true,
        }
    }

    #[test]
    fn a_cover_is_a_square_thumbnail_and_one_that_cannot_be_read_is_dropped() {
        let dir = std::env::temp_dir().join(format!("patina-volume-art-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let good = dir.join("cover.png");
        image::RgbaImage::from_pixel(300, 200, image::Rgba([200, 50, 50, 255]))
            .save(&good)
            .unwrap();
        let bad = dir.join("not-a-picture.png");
        std::fs::write(&bad, b"nope").unwrap();

        let mut covers = Covers::default();
        let mut players = vec![player(Some(good.clone())), player(Some(bad)), player(None)];
        let fresh = covers.resolve(&mut players);
        assert_eq!(fresh.len(), 1);
        assert_eq!(fresh[0].1.size, [SIDE as usize, SIDE as usize]);
        assert_eq!(players[0].art, Some(good));
        assert_eq!(players[1].art, None, "unreadable: the card stays as it was");
        assert_eq!(players[2].art, None);
        assert!(
            covers.resolve(&mut players).is_empty(),
            "read once, not at every poll"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
