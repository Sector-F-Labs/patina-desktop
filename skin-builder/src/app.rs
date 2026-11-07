//! The window: Patina's frame and header bar, the skins you've made on the left, and for the one
//! open an Edit page (the colours, with a live preview) and an Apply page (the folders in
//! `~/.config`, to put it in).

use std::path::PathBuf;

use egui::color_picker::{Alpha, color_edit_button_srgba};
use egui::{Color32, RichText, Ui};
use patina::frame::{self, TitleBar};
use patina::skin::{self, Skin};

use crate::doc::{self, FIELDS, SECTIONS, SkinDoc};
use crate::targets::{self, Target};

/// Where your skins are kept, `~/.config/patina-skin-builder/skins`.
const APP: &str = "patina-skin-builder";

#[derive(Clone, Copy, PartialEq)]
enum Page {
    Edit,
    Apply,
}

/// A skin in your folder.
struct Entry {
    path: PathBuf,
    name: String,
}

/// The skin open for editing.
struct Open {
    /// Its file, once saved.
    path: Option<PathBuf>,
    doc: SkinDoc,
    dirty: bool,
}

/// The state of the "new skin" row: a name, and what to start from.
struct New {
    name: String,
    /// `None` for a fresh dark skin, else an index into the built-ins.
    from: Option<usize>,
}

struct Applying {
    target: Target,
    chosen: bool,
}

enum Action {
    Quit,
    Save,
    Load(PathBuf),
    Create,
    Delete,
    Apply,
    Rescan,
}

pub struct App {
    dir: Option<PathBuf>,
    library: Vec<Entry>,
    open: Option<Open>,
    page: Page,
    new: New,
    applying: Vec<Applying>,
    filter: String,
    /// What the last save or apply did, shown at the bottom.
    message: Option<(bool, String)>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        patina::appearance::watch(&cc.egui_ctx);
        Skin::default_skin().apply(&cc.egui_ctx);
        let mut app = Self {
            dir: skin::app_dir(APP),
            library: Vec::new(),
            open: None,
            page: Page::Edit,
            new: New {
                name: String::new(),
                from: None,
            },
            applying: Vec::new(),
            filter: String::new(),
            message: None,
        };
        app.scan_library();
        app.scan_targets();
        app
    }

    fn scan_library(&mut self) {
        self.library.clear();
        let Some(dir) = &self.dir else { return };
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for path in entries.filter_map(|e| e.ok().map(|e| e.path())) {
            if path.extension().is_some_and(|e| e == "toml")
                && let Ok(text) = std::fs::read_to_string(&path)
                && let Ok(doc) = SkinDoc::parse(&text)
            {
                self.library.push(Entry {
                    path,
                    name: doc.name(),
                });
            }
        }
        self.library.sort_by_key(|e| e.name.to_lowercase());
    }

    /// Reads `~/.config` again, keeping which folders were ticked.
    fn scan_targets(&mut self) {
        let Some(config) = dirs::config_dir() else {
            return;
        };
        let ticked: Vec<String> = self
            .applying
            .iter()
            .filter(|a| a.chosen)
            .map(|a| a.target.name.clone())
            .collect();
        self.applying = targets::find(&config)
            .into_iter()
            .map(|target| Applying {
                chosen: ticked.contains(&target.name),
                target,
            })
            .collect();
    }

    fn say(&mut self, ok: bool, text: impl Into<String>) {
        self.message = Some((ok, text.into()));
    }

    /// The open skin's file name in its folder.
    fn file_for(&self, open: &Open) -> String {
        open.path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| doc::file_name(&open.doc.name()))
    }

    fn save(&mut self) -> bool {
        let (Some(dir), Some(open)) = (self.dir.clone(), self.open.as_ref()) else {
            return false;
        };
        let path = open
            .path
            .clone()
            .unwrap_or_else(|| dir.join(self.file_for(open)));
        let text = open.doc.text();
        let result = std::fs::create_dir_all(&dir)
            .and_then(|()| std::fs::write(&path, text))
            .map_err(|e| format!("{}: {e}", path.display()));
        match result {
            Ok(()) => {
                if let Some(open) = &mut self.open {
                    open.path = Some(path.clone());
                    open.dirty = false;
                }
                self.scan_library();
                self.say(true, format!("Saved {}", path.display()));
                true
            }
            Err(e) => {
                self.say(false, format!("Couldn't save: {e}"));
                false
            }
        }
    }

    fn create(&mut self) {
        let name = self.new.name.trim().to_string();
        if name.is_empty() {
            return;
        }
        let doc = match self.new.from {
            None => SkinDoc::new(&name, true),
            Some(i) => match SkinDoc::parse(skin::BUILT_IN[i].1) {
                Ok(mut doc) => {
                    doc.set_name(&name);
                    doc
                }
                Err(e) => return self.say(false, format!("Couldn't read that skin: {e}")),
            },
        };
        let taken = self
            .dir
            .as_ref()
            .is_some_and(|d| d.join(doc::file_name(&name)).exists());
        if taken {
            return self.say(false, format!("There's already a skin called {name}"));
        }
        self.open = Some(Open {
            path: None,
            doc,
            dirty: true,
        });
        self.new.name.clear();
        self.page = Page::Edit;
        self.save();
    }

    fn load(&mut self, path: PathBuf) {
        match std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|t| SkinDoc::parse(&t))
        {
            Ok(doc) => {
                self.open = Some(Open {
                    path: Some(path),
                    doc,
                    dirty: false,
                })
            }
            Err(e) => self.say(false, format!("{}: {e}", path.display())),
        }
    }

    fn delete(&mut self) {
        let Some(path) = self.open.as_ref().and_then(|o| o.path.clone()) else {
            return;
        };
        match std::fs::remove_file(&path) {
            Ok(()) => {
                self.open = None;
                self.scan_library();
                self.say(true, format!("Deleted {}", path.display()));
            }
            Err(e) => self.say(false, format!("{}: {e}", path.display())),
        }
    }

    /// Saves the open skin, then force-applies it to every ticked folder.
    fn apply(&mut self) {
        if !self.save() {
            return;
        }
        let Some(open) = &self.open else { return };
        let (file, text, name) = (self.file_for(open), open.doc.text(), open.doc.name());
        let mut done = Vec::new();
        let mut failed = Vec::new();
        for a in self.applying.iter().filter(|a| a.chosen) {
            match targets::apply(&a.target, &file, &text, &name) {
                Ok(()) => done.push(a.target.name.clone()),
                Err(e) => failed.push(e),
            }
        }
        self.scan_targets();
        if failed.is_empty() {
            self.say(
                true,
                format!(
                    "Applied {name} to {}. They show it when next started.",
                    done.join(", ")
                ),
            );
        } else {
            self.say(
                false,
                format!("Couldn't apply to everything: {}", failed.join("; ")),
            );
        }
    }

    fn menus(&self, ui: &mut Ui, actions: &mut Vec<Action>) {
        ui.menu_button("File", |ui| {
            if ui
                .add_enabled(
                    self.open.is_some(),
                    egui::Button::new("Save").shortcut_text("Ctrl+S"),
                )
                .clicked()
            {
                actions.push(Action::Save);
                ui.close();
            }
            if ui
                .add(egui::Button::new("Quit").shortcut_text("Ctrl+Q"))
                .clicked()
            {
                actions.push(Action::Quit);
                ui.close();
            }
        });
    }

    fn library_panel(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        ui.add_space(4.0);
        ui.label(RichText::new("Your skins").strong());
        let current = self.open.as_ref().and_then(|o| o.path.clone());
        egui::ScrollArea::vertical()
            .max_height(ui.available_height() - 150.0)
            .show(ui, |ui| {
                if self.library.is_empty() {
                    ui.weak("None yet. Make one below.");
                }
                for entry in &self.library {
                    let on = current.as_ref() == Some(&entry.path);
                    if ui.selectable_label(on, &entry.name).clicked() && !on {
                        actions.push(Action::Load(entry.path.clone()));
                    }
                }
            });
        ui.separator();
        ui.label(RichText::new("New skin").strong());
        ui.add(
            egui::TextEdit::singleline(&mut self.new.name)
                .hint_text("Name")
                .desired_width(f32::INFINITY),
        );
        let from = match self.new.from {
            None => "A fresh dark skin".to_string(),
            Some(i) => format!("Copy of {}", skin_name(skin::BUILT_IN[i].1)),
        };
        egui::ComboBox::from_id_salt("from")
            .selected_text(from)
            .width(ui.available_width() - 8.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut self.new.from, None, "A fresh dark skin");
                for (i, (_, text)) in skin::BUILT_IN.iter().enumerate() {
                    ui.selectable_value(
                        &mut self.new.from,
                        Some(i),
                        format!("Copy of {}", skin_name(text)),
                    );
                }
            });
        let ready = !self.new.name.trim().is_empty();
        if ui
            .add_enabled(ready, egui::Button::new("Create"))
            .on_hover_text("Makes the skin and opens it")
            .clicked()
        {
            actions.push(Action::Create);
        }
    }

    fn edit_page(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let Some(open) = &mut self.open else {
            ui.add_space(8.0);
            ui.weak("Pick a skin on the left, or make a new one.");
            return;
        };
        let mut changed = false;
        ui.columns(2, |cols| {
            let ui = &mut cols[0];
            egui::ScrollArea::vertical()
                .id_salt("editor")
                .auto_shrink(false)
                .show(ui, |ui| {
                    changed |= editor(ui, &mut open.doc);
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
                            actions.push(Action::Save);
                        }
                        if ui
                            .add_enabled(open.path.is_some(), egui::Button::new("Delete"))
                            .on_hover_text("Deletes the skin's file from your skins")
                            .clicked()
                        {
                            actions.push(Action::Delete);
                        }
                    });
                });
            preview(&mut cols[1], &open.doc);
        });
        if changed {
            open.dirty = true;
        }
    }

    fn apply_page(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let Some(open) = &self.open else {
            ui.add_space(8.0);
            ui.weak("Open a skin to apply it.");
            return;
        };
        ui.add_space(4.0);
        ui.label(format!(
            "Applying {} puts the skin in each ticked app's skins folder, replacing one there \
             with the same file name, and makes it that app's skin. An app shows it when it \
             next starts.",
            open.doc.name()
        ));
        ui.add_space(4.0);
        let count = self.applying.iter().filter(|a| a.chosen).count();
        let plural = if count == 1 { "" } else { "s" };
        if ui
            .add_enabled(
                count > 0,
                egui::Button::new(format!("Apply to {count} folder{plural}")),
            )
            .on_hover_text("Saves the skin and puts it in each ticked folder")
            .clicked()
        {
            actions.push(Action::Apply);
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.filter)
                    .hint_text("Filter")
                    .desired_width(200.0),
            );
            if ui.button("Tick the Patina apps").clicked() {
                for a in &mut self.applying {
                    a.chosen = a.target.patina;
                }
            }
            if ui.button("Tick none").clicked() {
                for a in &mut self.applying {
                    a.chosen = false;
                }
            }
            if ui
                .button("Rescan")
                .on_hover_text("Reads ~/.config again")
                .clicked()
            {
                actions.push(Action::Rescan);
            }
        });
        let words = self.filter.to_lowercase();
        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("targets")
                .striped(true)
                .num_columns(3)
                .show(ui, |ui| {
                    ui.label(RichText::new("Folder").strong());
                    ui.label(RichText::new("Its skin now").strong());
                    ui.label(RichText::new("").strong());
                    ui.end_row();
                    for a in self
                        .applying
                        .iter_mut()
                        .filter(|a| a.target.name.to_lowercase().contains(&words))
                    {
                        ui.checkbox(&mut a.chosen, &a.target.name);
                        match &a.target.skin {
                            Some(skin) => ui.label(skin),
                            None => ui.weak("—"),
                        };
                        if a.target.patina {
                            ui.weak("Patina app");
                        } else {
                            ui.weak("not known as one");
                        }
                        ui.end_row();
                    }
                });
        });
    }
}

fn skin_name(text: &str) -> String {
    SkinDoc::parse(text)
        .map(|d| d.name())
        .unwrap_or_else(|_| "a skin".into())
}

/// The fields: name, author, base, rounding, shadows, and each colour. True if any changed.
fn editor(ui: &mut Ui, skin: &mut SkinDoc) -> bool {
    let mut changed = false;
    egui::Grid::new("about").num_columns(2).show(ui, |ui| {
        ui.label("Name");
        let mut name = skin.name();
        if ui.text_edit_singleline(&mut name).changed() {
            skin.set_name(&name);
            changed = true;
        }
        ui.end_row();
        ui.label("Author");
        let mut author = skin.author();
        if ui.text_edit_singleline(&mut author).changed() {
            skin.set_author(&author);
            changed = true;
        }
        ui.end_row();
        ui.label("Base");
        let mut dark = skin.dark();
        ui.horizontal(|ui| {
            changed |= ui.selectable_value(&mut dark, true, "Dark").changed();
            changed |= ui.selectable_value(&mut dark, false, "Light").changed();
        });
        if dark != skin.dark() {
            skin.set_dark(dark);
        }
        ui.end_row();
        ui.label("Corner radius")
            .on_hover_text("Panels, menus and controls");
        let mut radius = skin.corner_radius().unwrap_or(6);
        if ui.add(egui::Slider::new(&mut radius, 0..=20)).changed() {
            skin.set_corner_radius(Some(radius));
            changed = true;
        }
        ui.end_row();
        ui.label("Shadows");
        let mut shadows = skin.shadows();
        if ui.checkbox(&mut shadows, "").changed() {
            skin.set_shadows(shadows);
            changed = true;
        }
        ui.end_row();
    });
    let base = doc::visuals(skin.dark());
    for (section, title) in SECTIONS {
        ui.add_space(6.0);
        ui.label(RichText::new(*title).strong());
        egui::Grid::new(*section).num_columns(3).show(ui, |ui| {
            for field in FIELDS.iter().filter(|f| f.section == *section) {
                let set = skin.colour(field);
                let mut colour = set.unwrap_or_else(|| doc::fallback(field, &base));
                ui.label(field.label).on_hover_text(field.hint);
                if color_edit_button_srgba(ui, &mut colour, Alpha::OnlyBlend).changed() {
                    skin.set_colour(field, colour);
                    changed = true;
                }
                ui.horizontal(|ui| {
                    ui.monospace(doc::hex(colour));
                    if set.is_none() {
                        ui.weak("(egui's)");
                    } else if ui
                        .small_button("↺")
                        .on_hover_text("Leave it out: egui's own colour for this base")
                        .clicked()
                    {
                        skin.clear_colour(field);
                        changed = true;
                    }
                });
                ui.end_row();
            }
            if *section == "window" {
                changed |= window_radius(ui, skin);
                ui.end_row();
            }
        });
    }
    changed
}

/// The window's corners: its own radius if the skin has one, else the main one's, else Patina's 15.
fn window_radius(ui: &mut Ui, skin: &mut SkinDoc) -> bool {
    let mut changed = false;
    let own = skin.window_radius();
    ui.label("Corner radius").on_hover_text(
        "The window's own corners. Left out, they follow the main corner radius, or 15 if \
         the skin has none",
    );
    ui.horizontal(|ui| {
        let mut radius = own.or(skin.corner_radius()).unwrap_or(15);
        if ui.add(egui::Slider::new(&mut radius, 0..=30)).changed() {
            skin.set_window_radius(Some(radius));
            changed = true;
        }
    });
    ui.horizontal(|ui| {
        if own.is_none() {
            ui.weak("(follows the main one)");
        } else if ui
            .small_button("↺")
            .on_hover_text("Leave it out: follow the main corner radius")
            .clicked()
        {
            skin.set_window_radius(None);
            changed = true;
        }
    });
    changed
}

/// A window drawn in the skin, with the usual controls, as the apps would show it.
fn preview(ui: &mut Ui, skin_doc: &SkinDoc) {
    let skin = match Skin::parse(&skin_doc.text()) {
        Ok(skin) => skin,
        Err(e) => {
            ui.colored_label(ui.visuals().error_fg_color, e);
            return;
        }
    };
    for warning in &skin.warnings {
        ui.colored_label(ui.visuals().warn_fg_color, warning);
    }
    let visuals = skin.visuals(doc::visuals(skin_doc.dark()));
    let style = skin.frame_style(&visuals);
    let radius = f32::from(style.radius);
    let round = egui::CornerRadius::same(style.radius);
    let top = egui::CornerRadius {
        nw: style.radius,
        ne: style.radius,
        ..Default::default()
    };
    egui::Frame::new()
        .fill(visuals.panel_fill)
        .stroke(egui::Stroke::new(1.0, style.border))
        .corner_radius(round)
        .show(ui, |ui| {
            let (bar, _) = ui
                .allocate_exact_size(egui::vec2(ui.available_width(), 32.0), egui::Sense::hover());
            ui.painter().rect_filled(bar, top, style.title_bar);
            ui.painter().text(
                bar.left_center() + egui::vec2(radius.max(10.0), 0.0),
                egui::Align2::LEFT_CENTER,
                skin.name.as_str(),
                egui::FontId::proportional(14.0),
                style.title_text,
            );
            for (i, glyph) in ["–", "▢", "✕"].iter().enumerate() {
                ui.painter().text(
                    bar.right_center() - egui::vec2(18.0 + 28.0 * i as f32, 0.0),
                    egui::Align2::CENTER_CENTER,
                    *glyph,
                    egui::FontId::proportional(14.0),
                    style.controls.icon,
                );
            }
            ui.scope(|ui| {
                *ui.visuals_mut() = visuals.clone();
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.add_space(10.0);
                    ui.vertical(sample);
                });
                ui.add_space(10.0);
            });
        });
}

/// What the preview window holds.
fn sample(ui: &mut Ui) {
    let v = ui.visuals().clone();
    ui.heading("Heading");
    ui.label("Normal text, and the body of most things.");
    ui.label(RichText::new("Weak text for counts and status").color(v.weak_text_color()));
    ui.horizontal(|ui| {
        ui.hyperlink_to("A link", "https://patina.example");
        ui.colored_label(v.warn_fg_color, "A warning");
        ui.colored_label(v.error_fg_color, "An error");
    });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let _ = ui.button("Button");
        let _ = ui.add_enabled(false, egui::Button::new("Disabled"));
        let mut on = true;
        ui.checkbox(&mut on, "On");
        let mut off = false;
        ui.checkbox(&mut off, "Off");
    });
    let mut value = 0.6_f32;
    ui.add(egui::Slider::new(&mut value, 0.0..=1.0));
    let mut text = String::from("A text field");
    ui.text_edit_singleline(&mut text);
    ui.add_space(4.0);
    egui::Frame::new()
        .fill(v.extreme_bg_color)
        .inner_margin(4.0)
        .show(ui, |ui| {
            for (i, row) in ["A list", "with a selected row", "and an ordinary one"]
                .iter()
                .enumerate()
            {
                let stripe = if i % 2 == 1 {
                    v.faint_bg_color
                } else {
                    Color32::TRANSPARENT
                };
                egui::Frame::new().fill(stripe).show(ui, |ui| {
                    let _ = ui.selectable_label(i == 1, *row);
                });
            }
        });
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        patina::frame::shadow::request(frame);
        let style = Skin::default_skin().frame_style(ui.visuals());
        let radius = frame::corner_radius(&ctx, style.radius);

        let mut actions = Vec::new();
        let (save, quit) = ctx.input(|i| {
            (
                i.modifiers.command && i.key_pressed(egui::Key::S),
                i.modifiers.command && i.key_pressed(egui::Key::Q),
            )
        });
        if save && self.open.is_some() {
            actions.push(Action::Save);
        }
        if quit {
            actions.push(Action::Quit);
        }
        egui::Panel::top("header")
            .frame(frame::header_frame(ui.style(), &style, radius))
            .show(ui, |ui| {
                TitleBar::new("Skin Builder", &style)
                    .menu_bar(true)
                    .span(frame::header_span(ui))
                    .show(ui, |ui| self.menus(ui, &mut actions), |_ui| {});
            });
        egui::Panel::bottom("status")
            .frame(
                egui::Frame::side_top_panel(ui.style()).corner_radius(egui::CornerRadius {
                    sw: radius,
                    se: radius,
                    ..Default::default()
                }),
            )
            .show(ui, |ui| match &self.message {
                Some((ok, text)) => {
                    let colour = if *ok {
                        ui.visuals().text_color()
                    } else {
                        ui.visuals().error_fg_color
                    };
                    ui.colored_label(colour, text);
                }
                None => {
                    ui.weak(match &self.dir {
                        Some(dir) => format!("Your skins are in {}", dir.display()),
                        None => "No home folder to keep skins in".to_string(),
                    });
                }
            });
        egui::Panel::left("library")
            .resizable(true)
            .default_size(210.0)
            .show(ui, |ui| self.library_panel(ui, &mut actions));
        egui::CentralPanel::default().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.page, Page::Edit, "Edit");
                ui.selectable_value(&mut self.page, Page::Apply, "Apply");
                if let Some(open) = &self.open {
                    let dirty = if open.dirty { " •" } else { "" };
                    ui.weak(format!("{}{dirty}", open.doc.name()));
                }
            });
            ui.separator();
            match self.page {
                Page::Edit => self.edit_page(ui, &mut actions),
                Page::Apply => self.apply_page(ui, &mut actions),
            }
        });

        for action in actions {
            match action {
                Action::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                Action::Save => {
                    self.save();
                }
                Action::Load(path) => self.load(path),
                Action::Create => self.create(),
                Action::Delete => self.delete(),
                Action::Apply => self.apply(),
                Action::Rescan => self.scan_targets(),
            }
        }
        frame::edges(&ctx, radius, style.border);
    }

    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        frame::clear_color(visuals, true)
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        frame::release_grab(ctx, raw_input);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Draws the editor and the preview for `doc`, as a frame of the window would.
    fn draw(doc: &mut SkinDoc) {
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            editor(ui, doc);
            preview(ui, doc);
        });
    }

    #[test]
    fn a_skin_of_any_shape_can_be_drawn() {
        draw(&mut SkinDoc::new("New", true));
        draw(&mut SkinDoc::new("New light", false));
        draw(&mut SkinDoc::parse("format = 1\nname = \"Bare\"\n").unwrap());
        for (_, text) in skin::BUILT_IN {
            draw(&mut SkinDoc::parse(text).unwrap());
        }
    }
}
