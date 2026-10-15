//! The desktop's menu: for now the skin menu Patina apps have, choosing
//! the skin of the whole shell (the bar's, which the other pieces follow).
//!
//! It's drawn on a transparent surface over the whole screen, so it can
//! sit at the pointer and stay on screen near the edges, and a click
//! anywhere else, Escape, or losing the keyboard closes it.

use std::path::PathBuf;

use egui::{Area, Frame, Id, Order, Pos2, RichText, Ui};
use patina::skin::{self, Skin};
use patina_shell::Shell;

/// The bar's skins folder, where the shell's own skins go.
const APP: &str = "patina-bar";
const WIDTH: f32 = 240.0;

pub struct DesktopMenu {
    /// Where it opens, in the surface's points.
    at: Pos2,
    skins: Vec<Skin>,
    /// The skin in use, by index into `skins`.
    current: usize,
    /// Problems loading skins or saving, and other notes.
    notes: Vec<String>,
    had_focus: bool,
    done: bool,
}

impl DesktopMenu {
    pub fn new(at: Pos2) -> Self {
        let dirs: Vec<_> = skin::app_dir(APP).into_iter().collect();
        let (skins, notes) = skin::load_all(skin::BUILT_IN, &dirs);
        let name = patina_shell::bar::skin().unwrap_or_else(|| skin::DEFAULT_SKIN.to_string());
        let current = skins.iter().position(|s| s.name == name).unwrap_or(0);
        Self {
            at,
            skins,
            current,
            notes,
            had_focus: false,
            done: false,
        }
    }

    fn choose(&mut self, index: usize, ctx: &egui::Context) {
        match patina_shell::bar::set_skin(&self.skins[index].name) {
            Ok(()) => {
                self.current = index;
                self.skins[index].apply(ctx);
            }
            Err(e) => self.notes.push(format!("Couldn't save the skin: {e}")),
        }
    }

    fn items(&mut self, ui: &mut Ui) {
        ui.set_width(WIDTH);
        ui.label(RichText::new("Skin").weak());
        let mut chosen = None;
        for (i, skin) in self.skins.iter().enumerate() {
            let response = ui.radio(i == self.current, &skin.name);
            let response = match (&skin.path, &skin.author) {
                (Some(path), _) => response.on_hover_text(path.display().to_string()),
                (None, Some(author)) => response.on_hover_text(format!("Built in, by {author}")),
                (None, None) => response.on_hover_text("Stock egui"),
            };
            if response.clicked() {
                chosen = Some(i);
            }
        }
        if let Some(i) = chosen {
            self.choose(i, ui.ctx());
        }
        ui.separator();
        let dir = skin::app_dir(APP);
        if let Some(dir) = &dir {
            let copy = ui
                .button("Copy built-in skins to folder")
                .on_hover_text(dir.display().to_string());
            if copy.clicked() {
                self.notes.push(match skin::export_built_ins(dir, skin::BUILT_IN) {
                    Ok(export) if export.copied.is_empty() => {
                        "The built-in skins are already in the skins folder".into()
                    }
                    Ok(export) => format!("Copied {} to the skins folder", export.copied.join(", ")),
                    Err(e) => format!("Couldn't copy skins: {e}"),
                });
            }
            if ui.button("Open skins folder").clicked() {
                let _ = std::fs::create_dir_all(dir);
                self.open(dir.clone());
            }
        }
        if let Some(path) = patina_shell::bar::config_path()
            && ui
                .button("Open bar settings")
                .on_hover_text(path.display().to_string())
                .clicked()
        {
            // Opened even before it exists, so there's something to edit.
            if !path.exists() {
                let _ = patina_shell::bar::set_skin(&self.skins[self.current].name);
            }
            self.open(path);
        }
        if !self.notes.is_empty() {
            ui.separator();
            for note in &self.notes {
                ui.label(RichText::new(note).small().weak());
            }
        }
    }

    /// Opens `path` in the desktop's app for it, and closes.
    fn open(&mut self, path: PathBuf) {
        if let Err(e) = patina::open::path(&path) {
            self.notes.push(format!("Couldn't open {}: {e}", path.display()));
            return;
        }
        self.done = true;
    }
}

impl Shell for DesktopMenu {
    fn setup(&mut self, ctx: &egui::Context) {
        self.skins[self.current].apply(ctx);
        patina::appearance::watch(ctx);
    }

    fn ui(&mut self, ui: &mut Ui, _: &str) {
        if ui.input(|i| i.focused) {
            self.had_focus = true;
        } else if self.had_focus {
            self.done = true;
            return;
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.done = true;
            return;
        }
        let ctx = ui.ctx().clone();
        let shown = Area::new(Id::new("desktop-menu"))
            .order(Order::Foreground)
            .fixed_pos(self.at)
            .constrain(true)
            .show(&ctx, |ui| Frame::menu(ui.style()).show(ui, |ui| self.items(ui)));
        let outside = ui.input(|i| {
            i.pointer.any_pressed()
                && i
                    .pointer
                    .interact_pos()
                    .is_some_and(|p| !shown.response.rect.contains(p))
        });
        if outside {
            self.done = true;
        }
    }

    fn finished(&self) -> bool {
        self.done
    }
}
