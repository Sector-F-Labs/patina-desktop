//! The window: Patina's frame and header bar, the message table, and the detail of
//! the one picked.

use std::collections::VecDeque;
use std::sync::mpsc;
use std::time::UNIX_EPOCH;

use egui_extras::{Column, TableBuilder};
use patina::frame::{self, TitleBar};
use patina::skin::{self, Skin};

use crate::compose::Compose;
use crate::explain;
use crate::monitor::{self, Event, Kind, Record};
use crate::send::Sender;

const HOME_SKIN: &str = "Default";

/// The most messages kept; the oldest go first.
const KEPT: usize = 20_000;

enum Action {
    Quit,
    SetSkin(String),
    Clear,
    TogglePause,
    Resend(u64),
    OpenSend,
}

struct Row {
    record: Record,
    /// What a filter searches, lower case.
    haystack: String,
}

pub struct App {
    skins: Vec<Skin>,
    skin_errors: Vec<String>,
    skin: Option<String>,
    rows: VecDeque<Row>,
    events: mpsc::Receiver<Event>,
    filter: String,
    /// Shown while paused: nothing newer than this sequence number.
    frozen: Option<u64>,
    follow: bool,
    selected: Option<u64>,
    closed: Option<String>,
    compose: Compose,
    sender: Sender,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, rules: Vec<String>) -> Self {
        patina::appearance::watch(&cc.egui_ctx);
        let dirs: Vec<_> = skin::app_dir("patina-dbug").into_iter().collect();
        let (skins, skin_errors) = skin::load_all(skin::BUILT_IN, &dirs);
        let (tx, events) = mpsc::channel();
        let ctx = cc.egui_ctx.clone();
        let sender = Sender::new(tx.clone(), {
            let ctx = ctx.clone();
            move || ctx.request_repaint()
        });
        monitor::watch(rules, tx, move || ctx.request_repaint());
        let app = Self {
            skins,
            skin_errors,
            skin: None,
            rows: VecDeque::new(),
            events,
            filter: String::new(),
            frozen: None,
            follow: true,
            selected: None,
            closed: None,
            compose: Compose::default(),
            sender,
        };
        app.skin().apply(&cc.egui_ctx);
        app
    }

    fn skin(&self) -> &Skin {
        let find = |name: &str| self.skins.iter().find(|s| s.name == name);
        find(self.skin.as_deref().unwrap_or(HOME_SKIN))
            .or_else(|| find(HOME_SKIN))
            .unwrap_or(&self.skins[0])
    }

    fn drain(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::Message(record) => {
                    let haystack = record.haystack().to_lowercase();
                    self.rows.push_back(Row {
                        record: *record,
                        haystack,
                    });
                    if self.rows.len() > KEPT {
                        self.rows.pop_front();
                    }
                }
                Event::Sent { what, outcome } => self.compose.done(what, outcome),
                Event::Closed(why) => self.closed = Some(why),
            }
        }
    }

    /// The rows the table shows: within the freeze, and matching every word of the filter.
    fn visible(&self) -> Vec<&Row> {
        let words: Vec<String> = self
            .filter
            .split_whitespace()
            .map(str::to_lowercase)
            .collect();
        self.rows
            .iter()
            .filter(|r| self.frozen.is_none_or(|f| r.record.seq <= f))
            .filter(|r| words.iter().all(|w| r.haystack.contains(w)))
            .collect()
    }

    fn menus(&self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        ui.menu_button("File", |ui| {
            if ui
                .add(egui::Button::new("Quit").shortcut_text("Ctrl+Q"))
                .clicked()
            {
                actions.push(Action::Quit);
                ui.close();
            }
        });
        ui.menu_button("View", |ui| {
            let label = if self.frozen.is_some() {
                "Resume"
            } else {
                "Pause"
            };
            if ui
                .add(egui::Button::new(label).shortcut_text("Space"))
                .clicked()
            {
                actions.push(Action::TogglePause);
                ui.close();
            }
            if ui.button("Clear").clicked() {
                actions.push(Action::Clear);
                ui.close();
            }
        });
        ui.menu_button("Message", |ui| {
            if ui
                .button("Send Message…")
                .on_hover_text("Write a call or signal and send it")
                .clicked()
            {
                actions.push(Action::OpenSend);
                ui.close();
            }
            let picked = self.selected;
            if ui
                .add_enabled(picked.is_some(), egui::Button::new("Resend Selected…"))
                .on_hover_text("Fill the sender from the picked message")
                .clicked()
            {
                if let Some(seq) = picked {
                    actions.push(Action::Resend(seq));
                }
                ui.close();
            }
        });
        ui.menu_button("Skins", |ui| {
            let current = self.skin().name.clone();
            for skin in &self.skins {
                if ui.radio(skin.name == current, &skin.name).clicked() {
                    actions.push(Action::SetSkin(skin.name.clone()));
                }
            }
            for error in &self.skin_errors {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
        });
    }

    fn table(&mut self, ui: &mut egui::Ui) {
        let rows = self.visible();
        let mut picked = None;
        let mut resend = None;
        let mut builder = TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .sense(egui::Sense::click())
            .column(Column::exact(80.0))
            .column(Column::exact(52.0))
            .column(Column::initial(90.0).at_least(40.0))
            .column(Column::initial(90.0).at_least(40.0))
            .column(Column::initial(260.0).at_least(60.0))
            .column(Column::remainder().at_least(80.0).clip(true));
        if self.follow && self.frozen.is_none() && !rows.is_empty() {
            builder = builder.scroll_to_row(rows.len() - 1, Some(egui::Align::BOTTOM));
        }
        builder
            .header(22.0, |mut header| {
                for (title, about) in [
                    ("Time", "When Dbug saw the message, in UTC, to the millisecond."),
                    ("Type", "call, reply, error or signal. Hover over one in the list for what it is."),
                    ("From", "Who sent it: the :1.N connection the bus gave the program, or org.freedesktop.DBus, the bus itself."),
                    ("To", "Who it's for. A signal that goes to everyone listening has no one here."),
                    ("Interface", "The group of methods and signals it belongs to."),
                    ("Member and arguments", "The method or signal's name, then what was passed or returned."),
                ] {
                    header.col(|ui| {
                        ui.strong(title).on_hover_text(about);
                    });
                }
            })
            .body(|body| {
                body.rows(20.0, rows.len(), |mut row| {
                    let r = &rows[row.index()].record;
                    row.set_selected(self.selected == Some(r.seq));
                    row.col(|ui| {
                        ui.monospace(time_of(r));
                    });
                    row.col(|ui| {
                        let colour = match r.kind {
                            Kind::Error => ui.visuals().error_fg_color,
                            Kind::Signal => ui.visuals().hyperlink_color,
                            _ => ui.visuals().text_color(),
                        };
                        ui.colored_label(colour, r.kind.label()).on_hover_text(explain::kind_meaning(r.kind));
                    });
                    row.col(|ui| {
                        ui.label(&r.sender);
                    });
                    row.col(|ui| {
                        ui.label(&r.destination);
                    });
                    row.col(|ui| {
                        ui.label(&r.interface);
                    });
                    row.col(|ui| {
                        ui.label(format!("{} {}", r.member, r.body));
                    });
                    let response = row.response();
                    if response.clicked() {
                        picked = Some(r.seq);
                    }
                    if matches!(r.kind, Kind::Call | Kind::Signal) {
                        let seq = r.seq;
                        response.context_menu(|ui| {
                            if ui.button("Resend…").clicked() {
                                resend = Some(seq);
                                ui.close();
                            }
                        });
                    }
                });
            });
        if let Some(seq) = picked {
            self.selected = Some(seq);
        }
        if let Some(seq) = resend {
            self.selected = Some(seq);
            self.resend(seq);
        }
    }

    fn resend(&mut self, seq: u64) {
        if let Some(row) = self.rows.iter().find(|r| r.record.seq == seq) {
            self.compose.resend(&row.record);
        }
    }

    fn detail(&mut self, ui: &mut egui::Ui) {
        let Some(r) = self
            .selected
            .and_then(|s| self.rows.iter().find(|row| row.record.seq == s))
            .map(|r| &r.record)
        else {
            ui.weak(
                "Pick a message to see all of it: its path, signature and arguments, and for a \
                 reply the call it answers.",
            );
            return;
        };
        let resendable = matches!(r.kind, Kind::Call | Kind::Signal);
        let seq = r.seq;
        if let Some(text) = explain::meaning(&r.interface, &r.member) {
            ui.label(text);
            ui.add_space(4.0);
        }
        egui::Grid::new("detail").num_columns(2).show(ui, |ui| {
            let line = |ui: &mut egui::Ui, name: &str, value: String| {
                if !value.is_empty() {
                    ui.weak(name);
                    ui.add(egui::Label::new(value).selectable(true));
                    ui.end_row();
                }
            };
            line(ui, "Type", r.kind.label().into());
            line(ui, "Serial", r.serial.to_string());
            line(
                ui,
                "Reply to",
                r.reply_serial.map(|s| s.to_string()).unwrap_or_default(),
            );
            line(ui, "From", r.sender.clone());
            line(ui, "To", r.destination.clone());
            line(ui, "Path", r.path.clone());
            line(ui, "Interface", r.interface.clone());
            line(ui, "Member", r.member.clone());
            line(ui, "Signature", r.signature.clone());
            line(
                ui,
                "Call",
                r.call_seq.map(|s| format!("#{s}")).unwrap_or_default(),
            );
            line(
                ui,
                "Took",
                r.latency
                    .map(|d| format!("{:.2} ms", d.as_secs_f64() * 1000.0))
                    .unwrap_or_default(),
            );
        });
        if !r.body.is_empty() {
            ui.separator();
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add(egui::Label::new(egui::RichText::new(&r.body).monospace()).selectable(true));
            });
        }
        if resendable
            && ui
                .button("Resend…")
                .on_hover_text(
                    "Open the sender filled in from this message, to change something and send it again",
                )
                .clicked()
        {
            self.resend(seq);
        }
    }
}

/// `HH:MM:SS.mmm`, UTC: the bus's clock for the table, not the user's timezone.
fn time_of(r: &Record) -> String {
    let d = r.at.duration_since(UNIX_EPOCH).unwrap_or_default();
    let s = d.as_secs() % 86_400;
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        s / 3600,
        s / 60 % 60,
        s % 60,
        d.subsec_millis()
    )
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // Dbug always draws its own frame, so it always asks for the shadow.
        patina::frame::shadow::request(frame);
        self.drain();
        let style = self.skin().frame_style(ui.visuals());
        let radius = frame::corner_radius(&ctx, style.radius);

        let mut actions = Vec::new();
        if ctx.input(|i| i.key_pressed(egui::Key::Space)) && !ctx.egui_wants_keyboard_input() {
            actions.push(Action::TogglePause);
        }
        egui::Panel::top("header")
            .frame(frame::header_frame(ui.style(), &style, radius))
            .show(ui, |ui| {
                TitleBar::new("Dbug", &style)
                    .menu_bar(true)
                    .span(frame::header_span(ui))
                    .show(ui, |ui| self.menus(ui, &mut actions), |_ui| {});
            });
        for action in actions {
            match action {
                Action::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                Action::SetSkin(name) => {
                    self.skin = Some(name);
                    self.skin().apply(&ctx);
                }
                Action::OpenSend => self.compose.open = true,
                Action::Resend(seq) => self.resend(seq),
                Action::Clear => {
                    self.rows.clear();
                    self.selected = None;
                }
                Action::TogglePause => {
                    self.frozen = match self.frozen {
                        Some(_) => None,
                        None => Some(self.rows.back().map_or(0, |r| r.record.seq)),
                    };
                }
            }
        }

        egui::Panel::bottom("status")
            .frame(
                egui::Frame::side_top_panel(ui.style()).corner_radius(egui::CornerRadius {
                    sw: radius,
                    se: radius,
                    ..Default::default()
                }),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let shown = self.visible().len();
                    ui.label(format!("{shown} of {} messages", self.rows.len()));
                    if self.frozen.is_some() {
                        ui.label("· paused");
                    }
                    if let Some(why) = &self.closed {
                        ui.colored_label(ui.visuals().error_fg_color, format!("· stopped: {why}"));
                    }
                });
            });
        if self.compose.open {
            egui::Panel::right("send")
                .resizable(true)
                .default_size(430.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .small_button("✕")
                                .on_hover_text("Close the sender")
                                .clicked()
                            {
                                self.compose.open = false;
                            }
                        });
                    });
                    self.compose.show(ui, &self.sender);
                });
        }
        egui::Panel::bottom("detail")
            .resizable(true)
            .default_size(220.0)
            .show(ui, |ui| self.detail(ui));
        egui::CentralPanel::default().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Filter");
                ui.add(
                    patina::dialog::text_edit(ui, &mut self.filter)
                        .hint_text("words, all must match")
                        .desired_width(320.0),
                )
                .on_hover_text(
                    "Shows only messages with every word you type, anywhere in them: a name, an \
                     interface, a member, an argument. Try a program's name, or a member like \
                     NameOwnerChanged.",
                );
                ui.checkbox(&mut self.follow, "Follow")
                    .on_hover_text("Keep the newest message in view as they arrive.");
                let paused = self.frozen.is_some();
                let pause = ui.button(if paused { "Resume" } else { "Pause" });
                if pause
                    .on_hover_text(
                        "Freeze the list to read it. Messages still arrive and are kept; Resume \
                         shows them.",
                    )
                    .clicked()
                {
                    self.frozen = if paused {
                        None
                    } else {
                        Some(self.rows.back().map_or(0, |r| r.record.seq))
                    };
                }
                if ui
                    .button("Clear")
                    .on_hover_text("Forget every message so far.")
                    .clicked()
                {
                    self.rows.clear();
                    self.selected = None;
                }
            });
            ui.separator();
            egui::CollapsingHeader::new("What am I looking at?")
                .default_open(true)
                .show(ui, |ui| {
                    explain::explained(ui, explain::WATCHING);
                });
            if self.rows.is_empty() {
                ui.add_space(6.0);
                ui.weak(
                    "Nothing has gone by yet. Messages show up here as programs talk; opening a \
                     program or changing the volume usually sets some off.",
                );
            } else if self.visible().is_empty() {
                ui.add_space(6.0);
                ui.weak("Nothing matches the filter. Clear it to see everything again.");
            }
            ui.separator();
            self.table(ui);
        });

        frame::edges(&ctx, radius, style.border);
    }

    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        frame::clear_color(visuals, true)
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        frame::release_grab(ctx, raw_input);
    }
}
