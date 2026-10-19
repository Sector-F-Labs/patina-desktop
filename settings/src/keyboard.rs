//! The Keyboard page (ADR-0068): the layouts Ctrl+Space steps through, in
//! order, added from those xkb offers, and key repeat, written into the
//! compositor's `[keyboard]`; and, from the compositor over D-Bus, which
//! layout is in use, to switch to another.

use std::sync::mpsc;

use egui::{RichText, Ui, vec2};
use patina_system::keyboard::{self, Entry, Status};

use crate::app::Message;
use crate::compositor_config::{self as config, Keyboard};

pub struct KeyboardPage {
    settings: Keyboard,
    /// What the compositor last said: `None` until it's answered, then
    /// `None` inside when nothing serves the layouts.
    status: Option<Option<Status>>,
    updates: mpsc::Receiver<Option<Status>>,
    /// The layouts xkb offers, once read.
    catalog: Option<&'static [Entry]>,
    read: mpsc::Receiver<&'static [Entry]>,
    /// What's typed to find a layout to add.
    search: String,
    /// What's typed to try key repeat.
    trial: String,
    /// The system's layouts, for when the compositor isn't there to say.
    system: Vec<(String, String)>,
}

impl KeyboardPage {
    pub fn new(ctx: &egui::Context) -> Self {
        Self {
            settings: config::read_keyboard(),
            status: None,
            updates: watch(ctx.clone()),
            catalog: None,
            read: read_catalog(ctx.clone()),
            search: String::new(),
            trial: String::new(),
            system: keyboard::system_layouts(),
        }
    }

    fn save(&mut self, message: &mut Option<Message>) {
        if let Err(e) = config::write_keyboard(&self.settings) {
            *message = Some(Message::error(format!(
                "Couldn't save the keyboard settings: {e}"
            )));
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, message: &mut Option<Message>) {
        while let Ok(status) = self.updates.try_recv() {
            self.status = Some(status);
        }
        if let Ok(catalog) = self.read.try_recv() {
            self.catalog = Some(catalog);
        }
        ui.heading("Keyboard");
        ui.add_space(8.0);
        self.layouts(ui, message);
        ui.add_space(16.0);
        self.repeat(ui, message);
    }

    /// What xkb calls a layout: from its list, else from the compositor,
    /// else its names.
    fn name(&self, code: &str, variant: &str) -> String {
        let listed = self
            .catalog
            .and_then(|c| c.iter().find(|e| e.code == code && e.variant == variant));
        if let Some(entry) = listed {
            return entry.name.clone();
        }
        let served = self.served().and_then(|s| {
            s.layouts
                .iter()
                .find(|l| l.code == code && l.variant == variant)
        });
        match served {
            Some(layout) => layout.name.clone(),
            None if variant.is_empty() => code.to_string(),
            None => format!("{code} ({variant})"),
        }
    }

    fn served(&self) -> Option<&Status> {
        self.status.as_ref().and_then(|s| s.as_ref())
    }

    /// The layouts: those in the config, or else the ones the compositor
    /// took from the system, which a change here copies into the config.
    fn layouts(&mut self, ui: &mut Ui, message: &mut Option<Message>) {
        ui.label(RichText::new("Layouts").strong());
        ui.label(
            RichText::new(
                "Ctrl+Space switches to the next, in this order. With more than one, the bar \
                 shows the one in use; click it to choose another.",
            )
            .small()
            .weak(),
        );
        ui.add_space(4.0);
        let from_system = self.settings.layouts.is_empty();
        let shown: Vec<(String, String)> = if from_system {
            // The system's file, all of it, else what the compositor took
            // from it.
            match self.served() {
                Some(s) if self.system.is_empty() => s
                    .layouts
                    .iter()
                    .map(|l| (l.code.clone(), l.variant.clone()))
                    .collect(),
                _ => self.system.clone(),
            }
        } else {
            self.settings.layouts.clone()
        };
        // Which of them is in use, when the compositor has these very ones
        // (the first four).
        let used = shown.len().min(keyboard::MOST_LAYOUTS);
        let in_use = self.served().and_then(|s| {
            let same = s.layouts.len() == used
                && s.layouts
                    .iter()
                    .zip(&shown)
                    .all(|(l, (code, variant))| l.code == *code && l.variant == *variant);
            same.then_some(s.current)
        });

        let mut edited = None;
        let mut switch = None;
        let count = shown.len();
        egui::Grid::new("layouts")
            .num_columns(4)
            .spacing(vec2(12.0, 6.0))
            .show(ui, |ui| {
                for (i, (code, variant)) in shown.iter().enumerate() {
                    let label = keyboard::label_for(code);
                    let font = egui::TextStyle::Body.resolve(ui.style());
                    // A rune the skin's font hasn't got would be a box.
                    let label = if ui.ctx().fonts_mut(|f| f.has_glyphs(&font, &label)) {
                        label
                    } else {
                        keyboard::plain_label(code)
                    };
                    let name = self.name(code, variant);
                    if i >= keyboard::MOST_LAYOUTS {
                        ui.label(RichText::new(label).weak());
                        ui.label(RichText::new(name).weak());
                        ui.label(RichText::new("Not used").weak());
                    } else {
                        ui.label(RichText::new(label).strong());
                        ui.label(name);
                        if in_use == Some(i) {
                            ui.label(RichText::new("In use").weak());
                        } else if in_use.is_some() {
                            if ui.small_button("Use").clicked() {
                                switch = Some(i);
                            }
                        } else {
                            ui.label("");
                        }
                    }
                    ui.horizontal(|ui| {
                        let mut list = shown.clone();
                        if ui
                            .add_enabled(i > 0, egui::Button::new("Up").small())
                            .clicked()
                        {
                            list.swap(i, i - 1);
                            edited = Some(list);
                        } else if ui
                            .add_enabled(i + 1 < count, egui::Button::new("Down").small())
                            .clicked()
                        {
                            list.swap(i, i + 1);
                            edited = Some(list);
                        } else if ui
                            .add_enabled(count > 1, egui::Button::new("Remove").small())
                            .clicked()
                        {
                            list.remove(i);
                            edited = Some(list);
                        }
                    });
                    ui.end_row();
                }
            });
        if count == 0 && self.status.is_some() {
            ui.label(RichText::new("US English, xkb's default.").weak());
        }
        if count > keyboard::MOST_LAYOUTS {
            ui.add_space(4.0);
            ui.colored_label(
                ui.visuals().warn_fg_color,
                "Only the first four are used: a keyboard holds four layouts at most. Move the \
                 ones you want up, or remove the others.",
            );
        }
        ui.add_space(6.0);
        if let Some(added) = self.add_menu(ui, &shown) {
            let mut list = shown.clone();
            list.push(added);
            edited = Some(list);
        }
        if from_system && count > 0 {
            ui.label(
                RichText::new(
                    "From the system's keyboard settings (/etc/default/keyboard). A change here \
                     keeps them in the desktop's own.",
                )
                .small()
                .weak(),
            );
        }
        ui.label(
            RichText::new("Changing the list goes back to the first layout.")
                .small()
                .weak(),
        );

        if let Some(list) = edited {
            self.settings.layouts = list;
            self.save(message);
        }
        if let Some(i) = switch {
            if let Some(Some(status)) = &mut self.status {
                status.current = i;
            }
            std::thread::spawn(move || {
                if let Err(e) = keyboard::set_current(i) {
                    eprintln!("patina-settings: can't switch keyboard layout: {e}");
                }
            });
        }
    }

    /// "Add a layout…": xkb's layouts and variants, narrowed by what's
    /// typed; the one clicked, if any.
    fn add_menu(&mut self, ui: &mut Ui, shown: &[(String, String)]) -> Option<(String, String)> {
        let mut added = None;
        if shown.len() >= keyboard::MOST_LAYOUTS {
            ui.add_enabled(false, egui::Button::new("Add a layout…"))
                .on_disabled_hover_text(
                    "A keyboard holds four layouts at most: remove one to add another.",
                );
            return None;
        }
        let Some(catalog) = self.catalog else {
            ui.add_enabled(false, egui::Button::new("Add a layout…"));
            return None;
        };
        egui::ComboBox::from_id_salt("add-layout")
            .selected_text("Add a layout…")
            .width(280.0)
            .height(320.0)
            .show_ui(ui, |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.search)
                        .hint_text("Search")
                        .desired_width(f32::INFINITY),
                )
                .request_focus();
                let words: Vec<String> = self
                    .search
                    .to_lowercase()
                    .split_whitespace()
                    .map(String::from)
                    .collect();
                let matches = catalog.iter().filter(|e| {
                    let name = e.name.to_lowercase();
                    !shown.iter().any(|(c, v)| *c == e.code && *v == e.variant)
                        && words
                            .iter()
                            .all(|w| name.contains(w.as_str()) || e.code == *w)
                });
                for entry in matches.take(200) {
                    if ui.selectable_label(false, &entry.name).clicked() {
                        added = Some((entry.code.clone(), entry.variant.clone()));
                    }
                }
            });
        if added.is_some() {
            self.search.clear();
        }
        added
    }

    /// How soon, and how fast, a held key repeats.
    fn repeat(&mut self, ui: &mut Ui, message: &mut Option<Message>) {
        ui.label(RichText::new("Key Repeat").strong());
        let mut save = false;
        egui::Grid::new("repeat").num_columns(2).show(ui, |ui| {
            ui.label("Delay");
            let slider = ui.add(
                egui::Slider::new(&mut self.settings.repeat_delay, 100..=1000)
                    .suffix(" ms")
                    .step_by(10.0),
            );
            // Saved when it's let go, not on every step of a drag.
            save |= slider.drag_stopped() || (slider.changed() && !slider.dragged());
            ui.end_row();
            ui.label("Rate");
            let slider = ui.add(
                egui::Slider::new(&mut self.settings.repeat_rate, 5..=60).suffix(" per second"),
            );
            save |= slider.drag_stopped() || (slider.changed() && !slider.dragged());
            ui.end_row();
        });
        ui.add(
            egui::TextEdit::singleline(&mut self.trial)
                .hint_text("Type here to try it")
                .desired_width(280.0),
        );
        if save {
            self.save(message);
        }
    }
}

/// Follows the compositor's layouts on a thread, sending each change.
fn watch(ctx: egui::Context) -> mpsc::Receiver<Option<Status>> {
    let (send, receive) = mpsc::channel();
    let _ = std::thread::Builder::new()
        .name("keyboard".into())
        .spawn(move || {
            keyboard::watch(|status| {
                let sent = send.send(status).is_ok();
                ctx.request_repaint();
                sent
            })
        });
    receive
}

/// Reads xkb's layouts on a thread, as there are hundreds.
fn read_catalog(ctx: egui::Context) -> mpsc::Receiver<&'static [Entry]> {
    let (send, receive) = mpsc::channel();
    let _ = std::thread::Builder::new()
        .name("xkb-layouts".into())
        .spawn(move || {
            let _ = send.send(keyboard::catalog());
            ctx.request_repaint();
        });
    receive
}
