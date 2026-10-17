//! The desktop's menu: the skin menu Patina apps have, choosing the skin
//! of the whole shell (the bar's, which the other pieces follow), and
//! the way into Settings, which chooses the wallpaper (ADR-0030, ADR-0038).
//!
//! It closes as every menu here does (`overlay.rs`).

use std::path::PathBuf;

use egui::{Pos2, RichText, Ui};
use patina::skin::{self, Skin};
use patina_shell::Shell;

use crate::overlay::{self, Place};

/// The bar's skins folder, where the shell's own skins go.
const APP: &str = "patina-bar";
const WIDTH: f32 = 240.0;
/// Settings (ADR-0038).
const SETTINGS: &str = "patina-settings";

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
        if ui
            .button("Wallpaper…")
            .on_hover_text("Choose it in Settings")
            .clicked()
        {
            self.settings(Some("wallpaper"));
        }
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
        ui.separator();
        if ui.button("Settings…").clicked() {
            self.settings(None);
        }
        if !self.notes.is_empty() {
            ui.separator();
            for note in &self.notes {
                ui.label(RichText::new(note).small().weak());
            }
        }
    }

    /// Opens Settings, at `page` if one is given, and closes. Patina's
    /// compositor starts it, so it's among the desktop's processes
    /// (ADR-0045); under any other, the menu does.
    fn settings(&mut self, page: Option<&str>) {
        let line = match page {
            Some(page) => format!("{SETTINGS} --page {page}"),
            None => SETTINGS.to_string(),
        };
        if patina_system::processes::spawn(&line, &[]).is_ok() {
            self.done = true;
            return;
        }
        let mut command = std::process::Command::new(SETTINGS);
        if let Some(page) = page {
            command.args(["--page", page]);
        }
        match command.spawn() {
            Ok(_) => self.done = true,
            Err(e) => self.notes.push(format!("Couldn't open Settings: {e}")),
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
        if overlay::closing(ui, &mut self.had_focus) {
            self.done = true;
            return;
        }
        let place = Place {
            at: self.at,
            ..Default::default()
        };
        overlay::show(ui, "desktop-menu", place, |ui| self.items(ui));
        if overlay::pressed_outside(ui) {
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
