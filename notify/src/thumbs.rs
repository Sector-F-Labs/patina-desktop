//! The pictures on cards (ADR-0143): a screenshot's file is a full screen of pixels, so each
//! is read and cut down on a thread of its own, and the card shows it when it's ready. The
//! cut-down pixels are kept too, for the picture under the pointer while a card is dragged.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};

use egui::{ColorImage, TextureHandle};
use image::RgbaImage;
use patina_shell::calloop::channel::Sender as Wake;

/// The most pixels along a thumbnail's longer side.
const SIDE: u32 = 640;

pub struct Thumb {
    pub texture: TextureHandle,
    pub rgba: Arc<RgbaImage>,
}

enum Slot {
    Loading,
    Ready(Thumb),
    /// A file that wouldn't load: not asked for again.
    Failed,
}

pub struct Thumbs {
    slots: HashMap<PathBuf, Slot>,
    sent: Sender<(PathBuf, Option<RgbaImage>)>,
    done: Receiver<(PathBuf, Option<RgbaImage>)>,
    wake: Wake<()>,
}

impl Thumbs {
    pub fn new(wake: Wake<()>) -> Self {
        let (sent, done) = mpsc::channel();
        Thumbs {
            slots: HashMap::new(),
            sent,
            done,
            wake,
        }
    }

    /// Takes in the pictures the threads have finished.
    pub fn poll(&mut self, ctx: &egui::Context) {
        while let Ok((path, image)) = self.done.try_recv() {
            let slot = match image {
                Some(image) => {
                    let size = [image.width() as usize, image.height() as usize];
                    let colours = ColorImage::from_rgba_unmultiplied(size, image.as_raw());
                    let texture =
                        ctx.load_texture(path.to_string_lossy(), colours, Default::default());
                    Slot::Ready(Thumb {
                        texture,
                        rgba: Arc::new(image),
                    })
                }
                // A file read again while it's being written fails: the card keeps the picture
                // it had, and the next change reads it once more.
                None => match self.slots.remove(&path) {
                    Some(Slot::Ready(thumb)) => Slot::Ready(thumb),
                    _ => Slot::Failed,
                },
            };
            self.slots.insert(path, slot);
        }
    }

    /// Reads `path` on a thread of its own; its picture comes back through [`Thumbs::poll`].
    fn read(&self, path: &Path) -> bool {
        let (path, sent, wake) = (path.to_path_buf(), self.sent.clone(), self.wake.clone());
        std::thread::Builder::new()
            .name("thumbnail".into())
            .spawn(move || {
                let image = image::open(&path)
                    .ok()
                    .map(|i| i.thumbnail(SIDE, SIDE).to_rgba8());
                let _ = sent.send((path, image));
                let _ = wake.send(());
            })
            .is_ok()
    }

    /// `path`'s thumbnail if it's ready; the first ask starts reading it.
    pub fn get(&mut self, path: &Path) -> Option<&Thumb> {
        if !self.slots.contains_key(path) {
            self.slots.insert(path.to_path_buf(), Slot::Loading);
            if !self.read(path) {
                return None;
            }
        }
        match self.slots.get(path) {
            Some(Slot::Ready(thumb)) => Some(thumb),
            _ => None,
        }
    }

    /// Reads `path` again, for a file that changed (a screenshot edited): the card keeps the
    /// picture it has until the new one is ready. A file that wouldn't load is tried again.
    pub fn reload(&mut self, path: &Path) {
        if matches!(self.slots.get(path), Some(Slot::Ready(_) | Slot::Failed)) {
            self.read(path);
        }
    }

    /// Forgets the pictures of cards that are gone, so their textures go.
    pub fn keep(&mut self, paths: &[&Path]) {
        self.slots
            .retain(|path, slot| matches!(slot, Slot::Loading) || paths.contains(&path.as_path()));
    }
}

/// When files were last changed, to notice an edit: the first look at a file only notes its time.
#[derive(Default)]
pub struct Stamps {
    seen: HashMap<PathBuf, Option<std::time::SystemTime>>,
}

impl Stamps {
    /// The `paths` whose modified time is not what it was at the last look. Files no longer
    /// asked about are forgotten.
    pub fn changed(&mut self, paths: &[&Path]) -> Vec<PathBuf> {
        self.seen.retain(|path, _| paths.contains(&path.as_path()));
        let mut changed = Vec::new();
        for path in paths {
            let time = std::fs::metadata(path).and_then(|m| m.modified()).ok();
            let before = self.seen.insert(path.to_path_buf(), time);
            if before.is_some_and(|before| before != time) {
                changed.push(path.to_path_buf());
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("notify-thumbs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    #[test]
    fn an_edited_file_is_noticed_once_and_a_new_look_only_notes_the_time() {
        let file = scratch("shot.png");
        std::fs::write(&file, "one").unwrap();
        let mut stamps = Stamps::default();
        assert!(stamps.changed(&[&file]).is_empty(), "the first look");
        assert!(stamps.changed(&[&file]).is_empty(), "nothing changed");
        std::thread::sleep(Duration::from_millis(30));
        std::fs::write(&file, "two, edited").unwrap();
        assert_eq!(stamps.changed(&[&file]), vec![file.clone()]);
        assert!(stamps.changed(&[&file]).is_empty(), "said once");
    }

    #[test]
    fn a_file_that_goes_or_comes_is_a_change_and_one_not_asked_about_is_forgotten() {
        let file = scratch("gone.png");
        std::fs::write(&file, "x").unwrap();
        let mut stamps = Stamps::default();
        stamps.changed(&[&file]);
        std::fs::remove_file(&file).unwrap();
        assert_eq!(stamps.changed(&[&file]), vec![file.clone()], "it went");
        stamps.changed(&[]);
        std::fs::write(&file, "back").unwrap();
        assert!(stamps.changed(&[&file]).is_empty(), "forgotten, so this is a first look");
    }
}
