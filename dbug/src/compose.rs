//! The message being written: where it goes, what it's called, and its arguments as a
//! form built from their types (`value.rs`).

use zbus::zvariant::Signature;

use crate::explain;
use crate::monitor::{Kind as Seen, Record};
use crate::send::{Kind, Outgoing, Sender};
use crate::value::{Edit, signature_of};

/// Types to start an argument from, as the Add argument menu offers them.
const TYPES: &[(&str, &str)] = &[
    ("String", "s"),
    ("Integer", "i"),
    ("Unsigned integer", "u"),
    ("Number", "d"),
    ("Boolean", "b"),
    ("Object path", "o"),
    ("List of strings", "as"),
    ("Dictionary of strings to variants", "a{sv}"),
    ("Variant", "v"),
];

pub struct Compose {
    pub open: bool,
    kind: Kind,
    destination: String,
    path: String,
    interface: String,
    member: String,
    args: Vec<Edit>,
    error: Option<String>,
    /// What came of each send, newest first.
    log: Vec<(String, Result<String, String>)>,
}

impl Default for Compose {
    fn default() -> Self {
        Self {
            open: false,
            kind: Kind::Call,
            destination: "org.freedesktop.DBus".into(),
            path: "/org/freedesktop/DBus".into(),
            interface: "org.freedesktop.DBus".into(),
            member: "ListNames".into(),
            args: Vec::new(),
            error: None,
            log: Vec::new(),
        }
    }
}

impl Compose {
    /// Fills the form from a message that went by, and opens it. Only a call or a signal can
    /// be sent again: a reply answers one particular call.
    pub fn resend(&mut self, r: &Record) {
        self.kind = match r.kind {
            Seen::Call => Kind::Call,
            Seen::Signal => Kind::Signal,
            Seen::Reply | Seen::Error => return,
        };
        self.destination = r.destination.clone();
        self.path = r.path.clone();
        self.interface = r.interface.clone();
        self.member = r.member.clone();
        self.args = r.args.iter().map(Edit::from_value).collect();
        self.error = None;
        self.open = true;
    }

    pub fn done(&mut self, what: String, outcome: Result<String, String>) {
        self.log.insert(0, (what, outcome));
        self.log.truncate(50);
    }

    pub fn show(&mut self, ui: &mut egui::Ui, sender: &Sender) {
        ui.heading("Send a message");
        explain::explained(ui, explain::SENDING);
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.kind, Kind::Call, "Method call")
                .on_hover_text("Ask a program to do something, and wait for its answer.");
            ui.radio_value(&mut self.kind, Kind::Signal, "Signal")
                .on_hover_text("Announce something to whoever is listening. There's no answer.");
        });
        egui::Grid::new("compose").num_columns(2).show(ui, |ui| {
            let field = |ui: &mut egui::Ui, name: &str, value: &mut String, hint: &str| {
                ui.label(name);
                ui.add(
                    egui::TextEdit::singleline(value)
                        .desired_width(f32::INFINITY)
                        .hint_text(hint),
                )
                .on_hover_text(hint);
                ui.end_row();
            };
            let to = if self.kind == Kind::Call {
                "The program to ask, by name: org.freedesktop.DBus, or a :1.42 connection."
            } else {
                "Empty sends it to everyone listening; a name sends it to that program alone."
            };
            field(ui, "To", &mut self.destination, to);
            field(
                ui,
                "Path",
                &mut self.path,
                "The object, such as /org/freedesktop/DBus.",
            );
            field(
                ui,
                "Interface",
                &mut self.interface,
                "The group of methods or signals, such as org.freedesktop.DBus.",
            );
            field(
                ui,
                "Member",
                &mut self.member,
                "The method or signal's name, such as ListNames.",
            );
        });
        ui.separator();
        ui.horizontal(|ui| {
            ui.strong("Arguments");
            ui.weak(format!(
                "type {}",
                if self.args.is_empty() {
                    "none".into()
                } else {
                    signature_of(&self.args)
                }
            ));
            ui.menu_button("+ Add", |ui| {
                for (name, sig) in TYPES {
                    if ui.button(*name).on_hover_text(*sig).clicked() {
                        if let Ok(sig) = sig.parse::<Signature>() {
                            self.args.push(Edit::blank(&sig));
                        }
                        ui.close();
                    }
                }
            })
            .response
            .on_hover_text(
                "Add an argument. The method's documentation says which types and in what order.",
            );
        });
        let mut gone = None;
        egui::ScrollArea::vertical()
            .id_salt("args")
            .max_height(220.0)
            .show(ui, |ui| {
                for (i, arg) in self.args.iter_mut().enumerate() {
                    ui.push_id(i, |ui| {
                        ui.horizontal_top(|ui| {
                            ui.weak(format!("{}", i + 1));
                            arg.show(ui);
                            if ui
                                .small_button("✕")
                                .on_hover_text("Remove this argument")
                                .clicked()
                            {
                                gone = Some(i);
                            }
                        });
                    });
                }
            });
        if let Some(i) = gone {
            self.args.remove(i);
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui.button("Send").on_hover_text("Send it now.").clicked() {
                self.error = match self.outgoing() {
                    Ok(out) => {
                        sender.send(out);
                        None
                    }
                    Err(e) => Some(e),
                };
            }
            if let Some(e) = &self.error {
                ui.colored_label(ui.visuals().error_fg_color, e);
            }
        });
        ui.separator();
        ui.strong("Answers");
        if self.log.is_empty() {
            ui.weak("Nothing sent yet.");
        }
        egui::ScrollArea::vertical().id_salt("log").show(ui, |ui| {
            for (what, outcome) in &self.log {
                ui.label(egui::RichText::new(what).strong());
                match outcome {
                    Ok(text) => ui.add(
                        egui::Label::new(egui::RichText::new(text).monospace()).selectable(true),
                    ),
                    Err(e) => ui.add(
                        egui::Label::new(egui::RichText::new(e).color(ui.visuals().error_fg_color))
                            .selectable(true),
                    ),
                };
                ui.add_space(4.0);
            }
        });
    }

    fn outgoing(&self) -> Result<Outgoing, String> {
        let need = |name: &str, v: &str| {
            if v.trim().is_empty() {
                Err(format!("{name} is empty."))
            } else {
                Ok(v.trim().to_string())
            }
        };
        let args = self
            .args
            .iter()
            .enumerate()
            .map(|(i, a)| a.to_value().map_err(|e| format!("Argument {}: {e}", i + 1)))
            .collect::<Result<_, _>>()?;
        Ok(Outgoing {
            kind: self.kind,
            destination: self.destination.trim().to_string(),
            path: need("Path", &self.path)?,
            interface: need("Interface", &self.interface)?,
            member: need("Member", &self.member)?,
            args,
        })
    }
}
