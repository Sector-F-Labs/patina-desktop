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
                None => Slot::Failed,
            };
            self.slots.insert(path, slot);
        }
    }

    /// `path`'s thumbnail if it's ready; the first ask starts reading it.
    pub fn get(&mut self, path: &Path) -> Option<&Thumb> {
        if !self.slots.contains_key(path) {
            self.slots.insert(path.to_path_buf(), Slot::Loading);
            let (path, sent, wake) = (path.to_path_buf(), self.sent.clone(), self.wake.clone());
            let spawned = std::thread::Builder::new()
                .name("thumbnail".into())
                .spawn(move || {
                    let image = image::open(&path)
                        .ok()
                        .map(|i| i.thumbnail(SIDE, SIDE).to_rgba8());
                    let _ = sent.send((path, image));
                    let _ = wake.send(());
                });
            if spawned.is_err() {
                return None;
            }
        }
        match self.slots.get(path) {
            Some(Slot::Ready(thumb)) => Some(thumb),
            _ => None,
        }
    }

    /// Forgets the pictures of cards that are gone, so their textures go.
    pub fn keep(&mut self, paths: &[&Path]) {
        self.slots
            .retain(|path, slot| matches!(slot, Slot::Loading) || paths.contains(&path.as_path()));
    }
}
