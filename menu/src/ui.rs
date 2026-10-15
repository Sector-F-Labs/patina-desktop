//! The desktop's menu: the skin menu Patina apps have, choosing the skin
//! of the whole shell (the bar's, which the other pieces follow), and the
//! wallpaper (ADR-0030).
//!
//! It's drawn on a transparent surface over the whole screen, so it can
//! sit at the pointer and stay on screen near the edges, and a click
//! anywhere else, Escape, or losing the keyboard closes it.

use std::path::PathBuf;

use egui::{Area, Frame, Id, Order, Pos2, RichText, Ui};
use patina::skin::{self, Skin};
use patina_shell::Shell;

use crate::wallpapers::{self, Wallpaper};

/// The bar's skins folder, where the shell's own skins go.
const APP: &str = "patina-bar";
const WIDTH: f32 = 240.0;
/// The wallpaper submenu scrolls past this.
const WALLPAPERS_HEIGHT: f32 = 420.0;

pub struct DesktopMenu {
    /// Where it opens, in the surface's points.
    at: Pos2,
    skins: Vec<Skin>,
    /// The skin in use, by index into `skins`.
    current: usize,
    wallpapers: Vec<Wallpaper>,
    /// The wallpaper in use; `None` for the default.
    wallpaper: Option<PathBuf>,
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
            wallpapers: wallpapers::find(),
            wallpaper: patina_shell::bar::wallpaper(),
            notes,
            had_focus: false,
            done: false,
        }
    }

    fn choose(&mut self, index: usize, ctx: &egui::Context) {
        match patina_shell::bar::set("skin", &self.skins[index].name) {
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
        ui.menu_button("Wallpaper", |ui| self.wallpaper_menu(ui));
        ui.separator();
        let dir = skin::app_dir(APP);
        if let Some(dir) = &dir {
            let copy = ui
                .button("Copy built-in skins to folder")
                .on_hover_text(dir.display().to_string());
            if copy.clicked() {
                self.notes
                    .push(match skin::export_built_ins(dir, skin::BUILT_IN) {
                        Ok(export) if export.copied.is_empty() => {
                            "The built-in skins are already in the skins folder".into()
                        }
                        Ok(export) => {
                            format!("Copied {} to the skins folder", export.copied.join(", "))
                        }
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
                let _ = patina_shell::bar::set("skin", &self.skins[self.current].name);
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

    /// The wallpapers: the default (the Sector F logo in the skin's
    /// colours), the images found, and the user's folder for more.
    fn wallpaper_menu(&mut self, ui: &mut Ui) {
        let mut chosen = None;
        if ui
            .radio(self.wallpaper.is_none(), "Sector F Labs")
            .on_hover_text("The default, in the skin's colours")
            .clicked()
        {
            chosen = Some(None);
        }
        egui::ScrollArea::vertical()
            .max_height(WALLPAPERS_HEIGHT)
            .show(ui, |ui| {
                for w in &self.wallpapers {
                    let on = self.wallpaper.as_deref() == Some(w.path.as_path());
                    if ui
                        .radio(on, &w.name)
                        .on_hover_text(w.path.display().to_string())
                        .clicked()
                    {
                        chosen = Some(Some(w.path.clone()));
                    }
                }
            });
        if let Some(path) = chosen {
            let value = path
                .as_deref()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            match patina_shell::bar::set("wallpaper", &value) {
                Ok(()) => self.wallpaper = path,
                Err(e) => self.notes.push(format!("Couldn't save the wallpaper: {e}")),
            }
        }
        if let Some(dir) = wallpapers::user_dir() {
            ui.separator();
            if ui
                .button("Open wallpapers folder")
                .on_hover_text(dir.display().to_string())
                .clicked()
            {
                let _ = std::fs::create_dir_all(&dir);
                self.open(dir);
            }
        }
    }

    /// Opens `path` in the desktop's app for it, and closes.
    fn open(&mut self, path: PathBuf) {
        if let Err(e) = patina::open::path(&path) {
            self.notes
                .push(format!("Couldn't open {}: {e}", path.display()));
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
        Area::new(Id::new("desktop-menu"))
            .order(Order::Foreground)
            .fixed_pos(self.at)
            .constrain(true)
            .show(&ctx, |ui| {
                Frame::menu(ui.style()).show(ui, |ui| self.items(ui))
            });
        // Outside the menu and its submenu: on the transparent background.
        // The layer is looked up after `input`, which holds the context's
        // lock that `layer_id_at` takes too.
        let pressed = ui.input(|i| {
            i.pointer
                .any_pressed()
                .then(|| i.pointer.interact_pos())
                .flatten()
        });
        let outside = pressed.is_some_and(|p| {
            ctx.layer_id_at(p)
                .is_none_or(|layer| layer.order == Order::Background)
        });
        if outside {
            self.done = true;
        }
    }

    fn finished(&self) -> bool {
        self.done
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One frame of the menu on a 1600x1000 screen, with `events`.
    fn frame(menu: &mut DesktopMenu, ctx: &egui::Context, events: Vec<egui::Event>) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                Pos2::ZERO,
                egui::vec2(1600.0, 1000.0),
            )),
            focused: true,
            events,
            ..Default::default()
        };
        let _ = ctx.run_ui(input, |ui| menu.ui(ui, ""));
    }

    fn press(at: Pos2) -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ]
    }

    /// A press used to look up the layer under it while holding egui's
    /// lock, which hung the menu, and the desktop under it, on any click.
    #[test]
    fn a_press_on_the_menu_keeps_it_and_one_elsewhere_closes_it() {
        let ctx = egui::Context::default();
        let mut menu = DesktopMenu::new(egui::pos2(400.0, 300.0));
        frame(&mut menu, &ctx, Vec::new());
        // The "Skin" heading: on the menu, but nothing to do.
        frame(&mut menu, &ctx, press(egui::pos2(415.0, 312.0)));
        assert!(!menu.done);
        frame(&mut menu, &ctx, press(egui::pos2(1200.0, 800.0)));
        assert!(menu.done);
    }
}
