//! The Web apps page (ADR-0133): the addresses installed as apps. Adding one writes its
//! `.desktop` entry, so it shows in the launcher and the bar; removing one takes the entry
//! and the app's logins away. The list itself is patina-webapp's.

use std::collections::HashSet;
use std::process::{Command, Stdio};
use std::sync::mpsc::{Receiver, Sender, channel};

use egui::{RichText, Ui};
use patina_webapp::{Engine, Store, WebApp};

use crate::app::Message;

/// A finished read of a site: which app, and what it came to.
type Fetched = (String, Result<WebApp, String>);

pub struct WebAppsPage {
    store: Option<Store>,
    apps: Vec<WebApp>,
    url: String,
    name: String,
    /// The slug asked to remove, until it's confirmed.
    removing: Option<String>,
    /// Apps whose colour and icon are being read, which is the network and takes a while.
    fetching: HashSet<String>,
    done: (Sender<Fetched>, Receiver<Fetched>),
}

impl WebAppsPage {
    pub fn new() -> Self {
        let store = Store::user().ok();
        // The entries are written again, so a change to how they're made (a window's app-id, which
        // an entry names) reaches the web apps already installed.
        if let Some(store) = &store {
            let _ = store.write_entries();
        }
        let apps = store.as_ref().map(Store::list).unwrap_or_default();
        Self {
            store,
            apps,
            url: String::new(),
            name: String::new(),
            removing: None,
            fetching: HashSet::new(),
            done: channel(),
        }
    }

    /// Reads `slug`'s site for its colour and icon, on a thread of its own.
    fn refresh(&mut self, ctx: &egui::Context, slug: &str) {
        let Some(store) = self.store.clone() else {
            return;
        };
        if !self.fetching.insert(slug.to_string()) {
            return;
        }
        let (tx, ctx, slug) = (self.done.0.clone(), ctx.clone(), slug.to_string());
        std::thread::spawn(move || {
            let result = store.refresh(&slug);
            let _ = tx.send((slug, result));
            ctx.request_repaint();
        });
    }

    pub fn ui(&mut self, ui: &mut Ui, message: &mut Option<Message>) {
        ui.heading("Web apps");
        ui.label(
            RichText::new(
                "A website installed as an app: it opens in a window of its own, with the \
                 desktop's frame, and is listed in the launcher and the bar. Each keeps its \
                 own logins.",
            )
            .small()
            .weak(),
        );
        ui.add_space(8.0);

        let Some(store) = self.store.clone() else {
            ui.label("There's no home folder to keep web apps in.");
            return;
        };
        while let Ok((slug, result)) = self.done.1.try_recv() {
            self.fetching.remove(&slug);
            self.apps = store.list();
            let name = self
                .apps
                .iter()
                .find(|a| a.slug == slug)
                .map(|a| a.name.clone());
            match (result, name) {
                (Ok(app), _) => {
                    let got = match (&app.color, &app.icon) {
                        (Some(_), Some(_)) => "its colour and icon",
                        (Some(_), None) => "its colour",
                        (None, Some(_)) => "its icon",
                        (None, None) => "nothing",
                    };
                    *message = Some(Message::note(format!("Read {got} for {}", app.name)));
                }
                (Err(e), Some(name)) => {
                    *message = Some(Message::error(format!("{name}: {e}")));
                }
                (Err(_), None) => {}
            }
        }

        if patina_webapp::find_webview().is_none() {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                "patina-webview isn't installed, so web apps can't open.",
            );
            ui.add_space(4.0);
        }

        ui.label(RichText::new("Add a web app").strong());
        let mut add = false;
        ui.horizontal(|ui| {
            let url = ui.add(
                patina::dialog::text_edit(ui, &mut self.url)
                    .hint_text("web.whatsapp.com")
                    .desired_width(260.0),
            );
            ui.add(
                patina::dialog::text_edit(ui, &mut self.name)
                    .hint_text("Name (optional)")
                    .desired_width(160.0),
            );
            add = ui.button("Add").clicked()
                || (url.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
        });
        if add {
            match store.add(&self.name, &self.url) {
                Ok(app) => {
                    *message = Some(Message::note(format!("Added {}", app.name)));
                    self.url.clear();
                    self.name.clear();
                    self.refresh(ui.ctx(), &app.slug);
                }
                Err(e) => *message = Some(Message::error(e)),
            }
            self.apps = store.list();
        }

        ui.add_space(12.0);
        if self.apps.is_empty() {
            ui.label(RichText::new("No web apps yet.").weak());
            return;
        }
        for app in self.apps.clone() {
            ui.horizontal(|ui| {
                // The frame's colour, as read from the site.
                if let Some(color) = app.color.as_deref().and_then(parse_color) {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
                    ui.painter().rect_filled(rect, 4.0, color);
                    ui.painter().rect_stroke(
                        rect,
                        4.0,
                        ui.visuals().widgets.noninteractive.bg_stroke,
                        egui::StrokeKind::Inside,
                    );
                }
                ui.vertical(|ui| {
                    ui.label(RichText::new(&app.name).strong());
                    let state = if self.fetching.contains(&app.slug) {
                        " · reading its icon and colour…"
                    } else {
                        ""
                    };
                    ui.label(RichText::new(format!("{}{state}", app.url)).small().weak());
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.removing.as_deref() == Some(&app.slug) {
                        if ui.button("Remove and forget logins").clicked() {
                            match store.remove(&app.slug) {
                                Ok(()) => {
                                    *message = Some(Message::note(format!("Removed {}", app.name)))
                                }
                                Err(e) => *message = Some(Message::error(e)),
                            }
                            self.removing = None;
                            self.apps = store.list();
                        }
                        if ui.button("Keep").clicked() {
                            self.removing = None;
                        }
                    } else {
                        if ui.button("Remove").clicked() {
                            self.removing = Some(app.slug.clone());
                        }
                        // Which engine opens it: WebKit is lighter and the desktop's own, but a
                        // site it can't run (or that crashes it) opens in Chromium.
                        let mut engine = app.engine;
                        egui::ComboBox::from_id_salt(("engine", &app.slug))
                            .selected_text(engine.name())
                            .width(90.0)
                            .show_ui(ui, |ui| {
                                for e in Engine::ALL {
                                    ui.selectable_value(&mut engine, e, e.name());
                                }
                            })
                            .response
                            .on_hover_text("The browser engine this web app opens in");
                        if engine != app.engine {
                            match store.set_engine(&app.slug, engine) {
                                Ok(()) => self.apps = store.list(),
                                Err(e) => *message = Some(Message::error(e)),
                            }
                        }
                        let busy = self.fetching.contains(&app.slug);
                        if ui
                            .add_enabled(!busy, egui::Button::new("Refresh"))
                            .on_hover_text("Read the site's icon and colour again")
                            .clicked()
                        {
                            self.refresh(ui.ctx(), &app.slug);
                        }
                        if ui.button("Open").clicked() {
                            let started = Command::new("patina-webapp")
                                .args(["launch", &app.slug])
                                .stdin(Stdio::null())
                                .spawn();
                            if let Err(e) = started {
                                *message = Some(Message::error(format!(
                                    "Couldn't start {}: {e}",
                                    app.name
                                )));
                            }
                        }
                    }
                });
            });
            ui.separator();
        }
    }
}

/// `#rrggbb` as a colour.
fn parse_color(hex: &str) -> Option<egui::Color32> {
    let hex = hex.strip_prefix('#')?;
    let n = u32::from_str_radix(hex, 16)
        .ok()
        .filter(|_| hex.len() == 6)?;
    Some(egui::Color32::from_rgb(
        (n >> 16) as u8,
        (n >> 8) as u8,
        n as u8,
    ))
}
