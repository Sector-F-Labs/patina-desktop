//! Each app's name and icon, from its `.desktop` entry
//! (`patina_system::apps`) and the icon theme, as the bar's tabs find
//! theirs. It reads files, so a thread of its own answers, and the window
//! asks each app id once.

use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc;

use egui::{ColorImage, TextureHandle};
use patina_system::apps::{entries, entry_for};

/// Icons' side, in pixels: twice the row's, for a sharp scaled one.
const SIDE: u32 = 32;

/// An app id's name and icon, once found.
#[derive(Default)]
pub struct Found {
    pub name: Option<String>,
    pub icon: Option<TextureHandle>,
}

pub struct Icons {
    found: HashMap<String, Found>,
    ask: mpsc::Sender<String>,
    answers: mpsc::Receiver<(String, Option<String>, Option<ColorImage>)>,
}

impl Icons {
    pub fn new(ctx: &egui::Context) -> Self {
        let (ask, asked) = mpsc::channel::<String>();
        let (answer, answers) = mpsc::channel();
        let ctx = ctx.clone();
        let _ = std::thread::Builder::new()
            .name("icons".into())
            .spawn(move || {
                let known = entries();
                for app_id in asked {
                    let entry = entry_for(&known, &app_id);
                    let name = entry.map(|e| e.name.clone());
                    let icon = match entry.and_then(|e| e.icon.as_deref()) {
                        Some(icon) => load(icon),
                        None => load(&app_id).or_else(|| load(&app_id.to_lowercase())),
                    };
                    if answer.send((app_id, name, icon)).is_err() {
                        return;
                    }
                    ctx.request_repaint();
                }
            });
        Self {
            found: HashMap::new(),
            ask,
            answers,
        }
    }

    /// What's known of `app_id` so far, asking for it the first time.
    pub fn get(&mut self, ctx: &egui::Context, app_id: &str) -> &Found {
        while let Ok((id, name, icon)) = self.answers.try_recv() {
            let icon = icon.map(|i| ctx.load_texture(format!("icon:{id}"), i, Default::default()));
            self.found.insert(id, Found { name, icon });
        }
        if !self.found.contains_key(app_id) {
            let _ = self.ask.send(app_id.to_string());
            self.found.insert(app_id.to_string(), Found::default());
        }
        &self.found[app_id]
    }
}

/// Icon `name` (a name in the theme, or a file), decoded.
fn load(name: &str) -> Option<ColorImage> {
    let path = Path::new(name);
    let file = if path.is_absolute() {
        path.exists().then(|| path.to_path_buf())
    } else {
        freedesktop_icons::lookup(name)
            .with_size(SIDE as u16)
            .with_theme(&patina_apps::icon::theme())
            .with_cache()
            .find()
    }?;
    let rgba = patina_apps::icon::render(&file, SIDE)?;
    Some(ColorImage::from_rgba_premultiplied(
        [SIDE as usize, SIDE as usize],
        &rgba,
    ))
}
