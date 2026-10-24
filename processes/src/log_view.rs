//! The Log tab (ADR-0090): the kernel's log since the machine started,
//! newest at the bottom and followed as it grows, filtered by how bad
//! (errors, warnings, everything), by what happened (crashes, the disks,
//! the GPU…) and by words. A line naming a process that's still running
//! opens its window from a right click.

use std::sync::mpsc;

use chrono::{Local, TimeZone};
use egui::{Align2, RichText, Sense, Ui, pos2, vec2};

use crate::kernel_log::{self, Entry, Part, Source, Update};
use crate::processes::{Do, Model, Pick};

const ROW: f32 = 22.0;
/// Lines kept; the oldest go first.
const KEPT: usize = 20_000;
const TIME_WIDTH: f32 = 128.0;
const LEVEL_WIDTH: f32 = 64.0;
const EVENT_WIDTH: f32 = 96.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Errors,
    Warnings,
    All,
}

impl Level {
    fn shows(self, e: &Entry) -> bool {
        e.event.is_some()
            || match self {
                Level::Errors => e.priority <= 3,
                Level::Warnings => e.priority <= 4,
                Level::All => true,
            }
    }
}

pub struct LogView {
    updates: mpsc::Receiver<Update>,
    pub entries: Vec<Entry>,
    source: Option<Source>,
    unreadable: Option<String>,
    pub level: Level,
    /// Only lines of this kind of event (`Event::label`).
    pub event: Option<&'static str>,
    pub filter: String,
}

impl LogView {
    pub fn new(ctx: &egui::Context) -> Self {
        Self {
            updates: kernel_log::start(ctx.clone()),
            entries: Vec::new(),
            source: None,
            unreadable: None,
            level: Level::Warnings,
            event: None,
            filter: String::new(),
        }
    }

    /// Takes what the reader sent since.
    pub fn take(&mut self) {
        while let Ok(u) = self.updates.try_recv() {
            match u {
                Update::Source(s) => self.source = Some(s),
                Update::Unreadable(e) => self.unreadable = Some(e),
                Update::Line(e) => self.entries.push(e),
            }
        }
        if self.entries.len() > KEPT {
            self.entries.drain(..self.entries.len() - KEPT);
        }
    }

    /// Shows only `event`'s lines, matching `words`, as the Overview's
    /// problems ask.
    pub fn show(&mut self, event: Option<&'static str>, words: &str) {
        self.level = Level::All;
        self.event = event;
        self.filter = words.to_string();
    }

    fn shown(&self) -> Vec<&Entry> {
        let words = self.filter.trim().to_lowercase();
        self.entries
            .iter()
            .filter(|e| self.level.shows(e))
            .filter(|e| match self.event {
                Some(kind) => e.event.as_ref().is_some_and(|ev| ev.label() == kind),
                None => true,
            })
            .filter(|e| words.is_empty() || e.message.to_lowercase().contains(&words))
            .collect()
    }

    pub fn ui(&mut self, ui: &mut Ui, model: &Model) -> Vec<Do> {
        let mut asked = Vec::new();
        self.explain(ui, &mut asked);
        ui.add_space(6.0);
        self.toolbar(ui);
        ui.add_space(6.0);
        if let Some(why) = &self.unreadable {
            ui.label(
                RichText::new(format!(
                    "The kernel's log can't be read here: {why}. On Linux, being in the adm \\
                     or systemd-journal group lets you read it through the journal."
                ))
                .color(ui.visuals().warn_fg_color),
            );
            return asked;
        }
        let shown = self.shown();
        if shown.is_empty() {
            let text = if self.source.is_none() {
                "Reading the kernel's log…"
            } else {
                "Nothing here with these filters."
            };
            ui.label(RichText::new(text).weak());
            return asked;
        }
        let spacing = ui.spacing().item_spacing.y;
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .stick_to_bottom(true)
                .show_rows(ui, ROW, shown.len(), |ui, range| {
                    for e in &shown[range] {
                        row(ui, e, model, &mut asked);
                    }
                });
        });
        ui.spacing_mut().item_spacing.y = spacing;
        asked
    }

    /// What the log is, where it's from, and what the chosen kind means,
    /// with links to the man pages that say more.
    fn explain(&self, ui: &mut Ui, asked: &mut Vec<Do>) {
        let mut parts = vec![
            Part::Text(
                "The kernel's own log since the machine started: what the kernel, its drivers \
                 and the hardware reported, and the programs it saw crash or had to end. Not \
                 what apps print; that's in their own logs. Error and warning are the kernel's \
                 own rating (its log levels, in ",
            ),
            Part::Man("syslog(2)"),
            Part::Text(
                "); the kinds beside them (Crash, GPU, Disk…) are the process manager's reading \
                 of the lines most worth knowing about, which the Overview's problems count. ",
            ),
        ];
        if let Some(s) = self.source {
            parts.extend(s.explain());
        }
        explained(ui, &parts, asked);
        if let Some(kind) = self.event {
            ui.add_space(6.0);
            let mut parts = vec![Part::Text(kernel_log::meaning(kind))];
            if let Some(page) = kernel_log::meaning_page(kind) {
                parts.extend([Part::Text(" More in "), Part::Man(page), Part::Text(".")]);
            }
            ui.label(RichText::new(kind).strong());
            explained(ui, &parts, asked);
        }
    }

    fn toolbar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            for (level, label) in [
                (Level::Errors, "Errors"),
                (Level::Warnings, "Warnings"),
                (Level::All, "Everything"),
            ] {
                ui.selectable_value(&mut self.level, level, label);
            }
            ui.separator();
            let kinds = [
                "Crash",
                "Out of memory",
                "Hung",
                "Disk",
                "GPU",
                "Heat",
                "Error",
            ];
            egui::ComboBox::from_id_salt("log event")
                .selected_text(self.event.unwrap_or("Any kind"))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.event, None, "Any kind");
                    for kind in kinds {
                        let count = self
                            .entries
                            .iter()
                            .filter(|e| e.event.as_ref().is_some_and(|ev| ev.label() == kind))
                            .count();
                        ui.selectable_value(
                            &mut self.event,
                            Some(kind),
                            format!("{kind} ({count})"),
                        );
                    }
                });
            ui.add(
                egui::TextEdit::singleline(&mut self.filter)
                    .hint_text("Filter by words")
                    .desired_width(220.0),
            );
            if !self.filter.is_empty() && ui.small_button("✕").on_hover_text("Clear").clicked() {
                self.filter.clear();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(s) = self.source {
                    ui.label(
                        RichText::new(format!(
                            "{} lines since start, {}",
                            self.entries.len(),
                            s.describe()
                        ))
                        .small()
                        .weak(),
                    );
                }
            });
        });
    }
}

/// A line: when, how bad, what happened, and what it says.
fn row(ui: &mut Ui, e: &Entry, model: &Model, asked: &mut Vec<Do>) {
    let v = ui.visuals().clone();
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
    if response.hovered() || response.context_menu_opened() {
        ui.painter()
            .rect_filled(rect, 2.0, v.widgets.hovered.weak_bg_fill);
    }
    let small = egui::TextStyle::Small.resolve(ui.style());
    let mono = egui::TextStyle::Monospace.resolve(ui.style());
    let y = rect.center().y;
    let mut x = rect.left() + 4.0;
    let painter = ui.painter_at(rect);
    painter.text(
        pos2(x, y),
        Align2::LEFT_CENTER,
        when(e.time),
        small.clone(),
        v.weak_text_color(),
    );
    x += TIME_WIDTH;
    let (level, colour) = match e.priority {
        0..=3 => ("error", v.error_fg_color),
        4 => ("warning", v.warn_fg_color),
        _ => ("", v.weak_text_color()),
    };
    painter.text(
        pos2(x, y),
        Align2::LEFT_CENTER,
        level,
        small.clone(),
        colour,
    );
    x += LEVEL_WIDTH;
    if let Some(event) = &e.event {
        painter.text(
            pos2(x, y),
            Align2::LEFT_CENTER,
            event.label(),
            small,
            v.strong_text_color(),
        );
    }
    x += EVENT_WIDTH;
    let message = egui::WidgetText::from(RichText::new(&e.message).font(mono)).into_galley(
        ui,
        Some(egui::TextWrapMode::Truncate),
        (rect.right() - x - 4.0).max(0.0),
        egui::TextStyle::Monospace,
    );
    painter.galley(pos2(x, y - message.size().y / 2.0), message, v.text_color());

    let response = response.on_hover_ui(|ui| {
        ui.set_max_width(640.0);
        ui.label(RichText::new(&e.message).monospace());
    });
    // The process it names, if it's still the one running with that PID.
    let process = e
        .event
        .as_ref()
        .and_then(|ev| ev.process())
        .and_then(|(name, pid)| {
            let running = model.process(pid)?;
            running.name.starts_with(name).then_some(pid)
        });
    response.context_menu(|ui| {
        if let Some(pid) = process {
            let pick = if model.app(pid).is_some() {
                Pick::App(pid)
            } else {
                Pick::Proc(pid)
            };
            if ui.button("Open Process").clicked() {
                asked.push(Do::Open(pick));
                ui.close();
            }
        }
        if ui.button("Copy Line").clicked() {
            asked.push(Do::Copy(e.message.clone()));
            ui.close();
        }
    });
    if response.double_clicked()
        && let Some(pid) = process
    {
        asked.push(Do::Open(Pick::Proc(pid)));
    }
}

/// An explanation at the size of the text around it, as wide as the
/// window; each man page a link that opens it in Reader.
pub fn explained(ui: &mut Ui, parts: &[Part], asked: &mut Vec<Do>) {
    ui.scope(|ui| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for part in parts {
                match part {
                    Part::Text(text) => {
                        ui.label(*text);
                    }
                    Part::Man(page) => {
                        if ui
                            .link(*page)
                            .on_hover_text(format!("Open the {page} man page in Reader"))
                            .clicked()
                        {
                            asked.push(Do::Man(page));
                        }
                    }
                }
            }
        });
    });
}

/// "Oct 24 14:03:09", in local time; nothing where the source gave none.
pub fn when(time: f64) -> String {
    if time <= 0.0 {
        return String::new();
    }
    Local
        .timestamp_opt(time as i64, 0)
        .single()
        .map_or(String::new(), |t| t.format("%b %d %H:%M:%S").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(priority: u8, message: &str) -> Entry {
        Entry {
            time: 0.0,
            priority,
            message: message.into(),
            event: kernel_log::classify(message, priority),
        }
    }

    #[test]
    fn levels_and_kinds_filter_and_events_always_show() {
        let (_, updates) = mpsc::channel();
        let mut view = LogView {
            updates,
            entries: vec![
                entry(6, "usb 1-1: new device"),
                entry(4, "something odd"),
                entry(3, "something broke"),
                entry(6, "drag-out[12]: segfault at 0 ip 1 sp 2 error 6 in x.so"),
            ],
            source: None,
            unreadable: None,
            level: Level::Errors,
            event: None,
            filter: String::new(),
        };
        let messages = |v: &LogView| {
            v.shown()
                .iter()
                .map(|e| e.message.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(messages(&view).len(), 2, "the error and the crash");
        view.level = Level::Warnings;
        assert_eq!(messages(&view).len(), 3);
        view.show(Some("Crash"), "");
        assert_eq!(
            messages(&view),
            ["drag-out[12]: segfault at 0 ip 1 sp 2 error 6 in x.so"]
        );
        view.show(None, "USB");
        assert_eq!(messages(&view), ["usb 1-1: new device"]);
    }
}
