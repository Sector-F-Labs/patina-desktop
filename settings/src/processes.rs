//! The Processes page (ADR-0045): what the compositor started, restarting
//! and stopping each, and a window following one's output. All through
//! `patina_system::processes`, on a thread, so a slow bus never holds the
//! window up.

use std::sync::mpsc;
use std::time::{Duration, SystemTime};

use egui::{RichText, Ui};
use patina_system::processes::{Process, Processes};

use crate::app::Message;

/// How often the list, and the log being followed, are asked for.
const POLL: Duration = Duration::from_millis(500);
/// Lines kept in the log window.
const LOG_LINES: usize = 2000;
/// This app, which can't restart itself without closing the window.
const SELF: &str = "patina-settings";

/// What the page asks of the worker.
enum Ask {
    /// Follow this process's log from its start, or none.
    Follow(Option<u32>),
    Restart(u32),
    Stop(u32),
}

/// What the worker sends back.
enum Update {
    /// The list; `None` when nothing serves it.
    List(Result<Option<Vec<Process>>, String>),
    Log {
        id: u32,
        lines: Vec<String>,
    },
    Failed(String),
}

/// The process whose log is open.
struct Log {
    id: u32,
    text: String,
    lines: usize,
}

pub struct ProcessesPage {
    list: Option<Result<Option<Vec<Process>>, String>>,
    log: Option<Log>,
    ask: mpsc::Sender<Ask>,
    updates: mpsc::Receiver<Update>,
}

impl ProcessesPage {
    pub fn new(ctx: &egui::Context) -> Self {
        let (ask, asked) = mpsc::channel();
        Self {
            list: None,
            log: None,
            ask,
            updates: work(ctx.clone(), asked),
        }
    }

    fn follow(&mut self, id: Option<u32>) {
        self.log = id.map(|id| Log {
            id,
            text: String::new(),
            lines: 0,
        });
        let _ = self.ask.send(Ask::Follow(id));
    }

    fn take_updates(&mut self, message: &mut Option<Message>) {
        while let Ok(update) = self.updates.try_recv() {
            match update {
                Update::List(list) => self.list = Some(list),
                Update::Log { id, lines } => {
                    let Some(log) = self.log.as_mut().filter(|l| l.id == id) else {
                        continue;
                    };
                    for line in lines {
                        log.text.push_str(&line);
                        log.text.push('\n');
                        log.lines += 1;
                    }
                    if log.lines > LOG_LINES {
                        let drop = log.lines - LOG_LINES;
                        let cut = log
                            .text
                            .match_indices('\n')
                            .nth(drop - 1)
                            .map_or(0, |(i, _)| i + 1);
                        log.text.drain(..cut);
                        log.lines = LOG_LINES;
                    }
                }
                Update::Failed(e) => *message = Some(Message::error(e)),
            }
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, message: &mut Option<Message>) {
        self.take_updates(message);
        // The uptimes tick.
        ui.ctx().request_repaint_after(Duration::from_secs(1));
        ui.heading("Processes");
        ui.label(
            "The desktop's pieces the compositor started. The wallpaper and the bar come back \
             when they exit. Click a name to read what it printed.",
        );
        ui.add_space(8.0);
        let list = match &self.list {
            None => return,
            Some(Err(e)) => {
                ui.label(RichText::new(format!("Can't ask the compositor: {e}")).weak());
                return;
            }
            Some(Ok(None)) => {
                ui.label(
                    RichText::new("The compositor isn't listing its processes here: this isn't Patina's desktop.")
                        .weak(),
                );
                return;
            }
            Some(Ok(Some(list))) => list.clone(),
        };

        let mut follow = None;
        egui::Grid::new("processes")
            .num_columns(6)
            .spacing([16.0, 6.0])
            .striped(true)
            .show(ui, |ui| {
                for title in ["Name", "Status", "PID", "Up", "Restarts", ""] {
                    ui.label(RichText::new(title).strong());
                }
                ui.end_row();
                for p in &list {
                    self.row(ui, p, &mut follow);
                    ui.end_row();
                }
            });
        if let Some(id) = follow {
            self.follow(Some(id));
        }
        self.log_window(ui.ctx(), &list);
    }

    fn row(&self, ui: &mut Ui, p: &Process, follow: &mut Option<u32>) {
        let shown = self.log.as_ref().is_some_and(|l| l.id == p.id);
        let mut about = p.command.clone();
        if p.kept {
            about.push_str("\nStarted again whenever it exits.");
        }
        if ui
            .selectable_label(shown, &p.name)
            .on_hover_text(about)
            .clicked()
        {
            *follow = Some(p.id);
        }
        let colour = if p.status.starts_with("gave up") {
            ui.visuals().error_fg_color
        } else if p.running() || p.status == "stopped" || p.status == "exited with 0" {
            ui.visuals().text_color()
        } else {
            ui.visuals().warn_fg_color
        };
        ui.colored_label(colour, &p.status);
        ui.label(p.pid.map(|pid| pid.to_string()).unwrap_or_default());
        ui.label(if p.running() {
            uptime(p.started)
        } else {
            String::new()
        });
        ui.label(p.restarts.to_string());
        ui.horizontal(|ui| {
            let own = p.name == SELF;
            let restart = if p.running() { "Restart" } else { "Start" };
            let clicked = ui
                .add_enabled(!own, egui::Button::new(restart))
                .on_disabled_hover_text(
                    "Settings can't restart itself; close it and open it again.",
                )
                .clicked();
            if clicked {
                let _ = self.ask.send(Ask::Restart(p.id));
            }
            if ui
                .add_enabled(p.running() && !own, egui::Button::new("Stop"))
                .clicked()
            {
                let _ = self.ask.send(Ask::Stop(p.id));
            }
        });
    }

    /// The followed process's output, in a window of its own over the page.
    fn log_window(&mut self, ctx: &egui::Context, list: &[Process]) {
        let Some(log) = &self.log else { return };
        let title = match list.iter().find(|p| p.id == log.id) {
            Some(p) => format!("{} — output", p.name),
            None => "Output".to_string(),
        };
        let mut open = true;
        egui::Window::new(title)
            .id(egui::Id::new("process-log"))
            .open(&mut open)
            .default_size([620.0, 340.0])
            .resizable(true)
            .show(ctx, |ui| {
                if log.text.is_empty() {
                    ui.label(RichText::new("Nothing printed yet.").weak());
                }
                egui::ScrollArea::both()
                    .stick_to_bottom(true)
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut log.text.as_str())
                                .font(egui::TextStyle::Monospace)
                                .desired_width(f32::INFINITY),
                        );
                    });
            });
        if !open {
            self.follow(None);
        }
    }
}

/// How long since `started`: "42s", "5m 3s", "2h 10m".
fn uptime(started: SystemTime) -> String {
    let s = SystemTime::now()
        .duration_since(started)
        .map_or(0, |d| d.as_secs());
    match s {
        0..60 => format!("{s}s"),
        60..3600 => format!("{}m {}s", s / 60, s % 60),
        _ => format!("{}h {}m", s / 3600, s % 3600 / 60),
    }
}

/// Carries out what the page asks, and polls the list and the followed
/// log, on a thread, sending what changes.
fn work(ctx: egui::Context, asked: mpsc::Receiver<Ask>) -> mpsc::Receiver<Update> {
    let (send, receive) = mpsc::channel();
    let _ = std::thread::Builder::new()
        .name("processes".into())
        .spawn(move || {
            let mut bus: Option<Processes> = None;
            let mut last = None;
            // The process followed, and the line to ask from next.
            let mut following: Option<(u32, u64)> = None;
            loop {
                let mut asks = Vec::new();
                match asked.recv_timeout(POLL) {
                    Ok(ask) => asks.push(ask),
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                }
                asks.extend(asked.try_iter());

                if bus.is_none() {
                    match Processes::connect() {
                        Ok(b) => bus = Some(b),
                        Err(e) => {
                            let _ = send.send(Update::List(Err(e)));
                            ctx.request_repaint();
                            std::thread::sleep(POLL * 4);
                            continue;
                        }
                    }
                }
                let Some(bus) = &bus else { continue };
                for ask in asks {
                    let done = match ask {
                        Ask::Follow(id) => {
                            following = id.map(|id| (id, 0));
                            Ok(())
                        }
                        Ask::Restart(id) => bus.restart(id),
                        Ask::Stop(id) => bus.stop(id),
                    };
                    if let Err(e) = done {
                        let _ = send.send(Update::Failed(e));
                    }
                }

                let now = match bus.served() {
                    Ok(true) => bus.list().map(Some),
                    Ok(false) => Ok(None),
                    Err(e) => Err(e),
                };
                let mut changed = last.as_ref() != Some(&now);
                if changed {
                    last = Some(now.clone());
                    if send.send(Update::List(now)).is_err() {
                        return;
                    }
                }
                if let Some((id, from)) = &mut following
                    && let Ok((next, lines)) = bus.log(*id, *from)
                    && !lines.is_empty()
                {
                    *from = next;
                    changed = true;
                    if send.send(Update::Log { id: *id, lines }).is_err() {
                        return;
                    }
                }
                if changed {
                    ctx.request_repaint();
                }
            }
        });
    receive
}
