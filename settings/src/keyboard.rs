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
    /// The shortcuts: the config's `[bindings]`, else the compositor's
    /// defaults, which the first change copies into the config.
    bindings: Vec<(String, String)>,
    /// Whether the config has a `[bindings]` section.
    custom: bool,
    /// The shortcut being added.
    adding: (String, String),
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
            bindings: Vec::new(),
            custom: false,
            adding: (String::new(), String::new()),
        }
        .with_bindings()
    }

    fn with_bindings(mut self) -> Self {
        let read = config::read_bindings();
        self.custom = read.is_some();
        self.bindings = read.unwrap_or_else(|| {
            config::DEFAULT_BINDINGS
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        });
        self
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
        ui.add_space(16.0);
        self.shortcuts(ui, message);
    }

    /// Writes the shortcuts that read as key combinations, once each.
    fn save_bindings(&mut self, message: &mut Option<Message>) {
        let mut seen = Vec::new();
        let list: Vec<(String, String)> = self
            .bindings
            .iter()
            .filter(|(k, c)| config::valid_combo(k) && !c.trim().is_empty())
            .filter(|(k, _)| {
                let fresh = !seen.contains(&k.to_lowercase());
                seen.push(k.to_lowercase());
                fresh
            })
            .cloned()
            .collect();
        self.custom = true;
        if let Err(e) = config::write_bindings(Some(&list)) {
            *message = Some(Message::error(format!("Couldn't save the shortcuts: {e}")));
        }
    }

    /// The keys that run commands: `[bindings]` in the compositor's config.
    fn shortcuts(&mut self, ui: &mut Ui, message: &mut Option<Message>) {
        ui.label(RichText::new("Shortcuts").strong());
        ui.label(
            RichText::new(
                "Keys that run a command: modifiers (Super, Alt, Ctrl, Shift) and a key by its \
                 name, like Super+Return or Alt+F5. patina-raise focuses an app's window, or \
                 starts the app if none is open.",
            )
            .small()
            .weak(),
        );
        ui.add_space(4.0);
        let mut save = false;
        let mut remove = None;
        for (i, (combo, command)) in self.bindings.iter_mut().enumerate() {
            let bad = !config::valid_combo(combo);
            ui.horizontal(|ui| {
                let (keys, run) = shortcut_fields(ui, combo, command, bad, ("", ""));
                save |= (keys.lost_focus() && keys.changed())
                    || (run.lost_focus() && run.changed())
                    || (keys.lost_focus() && bad);
                if ui
                    .add_sized([SHORTCUT_BUTTON, 0.0], egui::Button::new("Remove"))
                    .clicked()
                {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            self.bindings.remove(i);
            save = true;
        }
        if save {
            self.save_bindings(message);
        }
        ui.add_space(4.0);
        let ready = config::valid_combo(&self.adding.0) && !self.adding.1.trim().is_empty();
        let mut add = false;
        ui.horizontal(|ui| {
            let (keys, run) = shortcut_fields(
                ui,
                &mut self.adding.0,
                &mut self.adding.1,
                false,
                ("Super+E", "Command to run"),
            );
            let enter = (keys.lost_focus() || run.lost_focus())
                && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let button = ui.add_enabled(
                ready,
                egui::Button::new("Add").min_size(vec2(SHORTCUT_BUTTON, 0.0)),
            );
            add = button.clicked() || (enter && ready);
        });
        if add {
            let (combo, command) = std::mem::take(&mut self.adding);
            self.bindings
                .push((combo.trim().to_string(), command.trim().to_string()));
            self.save_bindings(message);
        }
        if self.custom {
            ui.add_space(4.0);
            if ui.button("Restore the defaults").clicked() {
                if let Err(e) = config::write_bindings(None) {
                    *message = Some(Message::error(format!("Couldn't save the shortcuts: {e}")));
                } else {
                    self.custom = false;
                    self.bindings = config::DEFAULT_BINDINGS
                        .iter()
                        .map(|(k, v)| (k.to_string(), v.to_string()))
                        .collect();
                }
            }
        }
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
                            if ui.button("Use").clicked() {
                                switch = Some(i);
                            }
                        } else {
                            ui.label("");
                        }
                    }
                    ui.horizontal(|ui| {
                        let mut list = shown.clone();
                        if ui.add_enabled(i > 0, egui::Button::new("Up")).clicked() {
                            list.swap(i, i - 1);
                            edited = Some(list);
                        } else if ui
                            .add_enabled(i + 1 < count, egui::Button::new("Down"))
                            .clicked()
                        {
                            list.swap(i, i + 1);
                            edited = Some(list);
                        } else if ui
                            .add_enabled(count > 1, egui::Button::new("Remove"))
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
                    patina::dialog::text_edit(ui, &mut self.search)
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
            patina::dialog::text_edit(ui, &mut self.trial)
                .hint_text("Type here to try it")
                .desired_width(280.0),
        );
        if save {
            self.save(message);
        }
    }
}

/// The width of a shortcut row's key field, and of its button.
const SHORTCUT_KEYS: f32 = 190.0;
const SHORTCUT_BUTTON: f32 = 80.0;

/// A shortcut's key and command fields, the command taking what the row's
/// key field and button leave.
fn shortcut_fields(
    ui: &mut Ui,
    keys: &mut String,
    command: &mut String,
    bad: bool,
    hints: (&str, &str),
) -> (egui::Response, egui::Response) {
    let gap = ui.spacing().item_spacing.x;
    let rest = ui.available_width() - SHORTCUT_KEYS - SHORTCUT_BUTTON - 2.0 * gap;
    let mut field = patina::dialog::text_edit(ui, keys)
        .hint_text(hints.0)
        .desired_width(SHORTCUT_KEYS);
    if bad {
        field = field.text_color(ui.visuals().error_fg_color);
    }
    let first = ui.add_sized([SHORTCUT_KEYS, 0.0], field);
    let width = rest.max(120.0);
    let second = patina::dialog::text_edit(ui, command)
        .hint_text(hints.1)
        .desired_width(width);
    (first, ui.add_sized([width, 0.0], second))
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
