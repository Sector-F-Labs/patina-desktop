//! A process's own window (ADR-0089): a real window, an egui viewport in
//! Patina's frame as Files' Quick Look is, so several can be open and set
//! side by side to compare. Its graphs from when it was opened, what it's
//! doing and waiting in, its memory broken down, and, for an app, the
//! processes it's made of; then its threads, network connections, open
//! files and environment (`details.rs`). End, Kill and Switch To on the
//! title bar's right, the actions that matter most here.

use std::time::{SystemTime, UNIX_EPOCH};

use egui::{RichText, Sense, Ui, vec2};
use patina::frame::{self, FrameStyle, TitleBar};

use crate::details::{Details, Memory};
use crate::graph::{self, Line, Scale, Series};
use crate::groups::Totals;
use crate::icons::Icons;
use crate::performance::{SPAN, duration, second_colour};
use crate::processes::{Do, Model, Pick, app_name};
use crate::sample::{Proc, Signal};

/// Its size to begin with.
pub const SIZE: [f32; 2] = [720.0, 820.0];
const GRAPH: f32 = 96.0;
/// Threads listed, the busiest first.
const THREADS: usize = 24;

pub struct ProcessWindow {
    pub pick: Pick,
    /// What it was called when opened, kept once it's gone.
    name: String,
    cpu: Series,
    memory: Series,
    disk: Series,
    gpu: Series,
    /// It has exited since the window opened.
    gone: bool,
}

impl ProcessWindow {
    pub fn new(pick: Pick, model: &Model, icons: &mut Icons, ctx: &egui::Context) -> Self {
        Self {
            pick,
            name: model.describe(pick, icons, ctx),
            cpu: Series::default(),
            memory: Series::default(),
            disk: Series::default(),
            gpu: Series::default(),
            gone: false,
        }
    }

    pub fn pid(&self) -> u32 {
        match self.pick {
            Pick::App(p) | Pick::Proc(p) => p,
        }
    }

    pub fn id(&self) -> egui::ViewportId {
        egui::ViewportId::from_hash_of(("process", self.pick))
    }

    pub fn title(&self) -> String {
        self.name.clone()
    }

    /// Its use in `model`'s reading, at `at` seconds; an app's is all its
    /// processes'.
    pub fn push(&mut self, at: f64, model: &Model) {
        let totals = match self.pick {
            Pick::App(pid) => model.app(pid).map(|a| a.totals),
            Pick::Proc(pid) => model.process(pid).map(Totals::of),
        };
        let Some(t) = totals else {
            self.gone = true;
            return;
        };
        self.cpu.push(at, f64::from(t.cpu));
        self.memory.push(at, t.memory as f64);
        self.disk.push(at, t.disk);
        self.gpu.push(at, f64::from(t.gpu));
    }

    /// The window's contents, in Patina's frame.
    pub fn ui(
        &self,
        ui: &mut Ui,
        model: &Model,
        icons: &mut Icons,
        style: &FrameStyle,
        custom: bool,
        now: f64,
    ) -> Vec<Do> {
        let ctx = ui.ctx().clone();
        let radius = if custom {
            frame::corner_radius(&ctx, style.radius)
        } else {
            0
        };
        let pid = self.pid();
        let proc = model.process(pid).filter(|_| !self.gone);
        let mut asked = Vec::new();

        egui::Panel::top("process-header")
            .frame(frame::header_frame(ui.style(), style, radius))
            .show(ui, |ui| {
                TitleBar::new(&self.name, style).custom(custom).show(
                    ui,
                    |_| {},
                    |ui| {
                        let alive = proc.is_some_and(|p| !p.kernel);
                        // Right to left.
                        if ui
                            .add_enabled(alive, egui::Button::new("Kill"))
                            .on_hover_text("End it at once (SIGKILL)")
                            .clicked()
                        {
                            asked.push(Do::Signal(self.pick, Signal::Kill));
                        }
                        if ui
                            .add_enabled(alive, egui::Button::new("End"))
                            .on_hover_text("Ask it to end (SIGTERM)")
                            .clicked()
                        {
                            asked.push(Do::Signal(self.pick, Signal::End));
                        }
                        if let Pick::App(pid) = self.pick
                            && ui
                                .add_enabled(alive, egui::Button::new("Switch To"))
                                .clicked()
                        {
                            asked.push(Do::Switch(pid));
                        }
                    },
                );
            });
        egui::Panel::bottom("process-status")
            .frame(
                egui::Frame::side_top_panel(ui.style())
                    .inner_margin(egui::Margin::symmetric(10, 4))
                    .corner_radius(egui::CornerRadius {
                        sw: radius,
                        se: radius,
                        ..Default::default()
                    }),
            )
            .show(ui, |ui| {
                let text = match proc {
                    Some(p) => format!("PID {pid} · {} · started {} ago", p.user, ago(p.started)),
                    None => format!("PID {pid} · ended"),
                };
                ui.label(RichText::new(text).small().weak());
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(ui.style()).inner_margin(16))
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let details = model.snapshot.details.get(&pid);
                    match proc {
                        Some(p) => summary(ui, p, details),
                        None => {
                            ui.label(
                                RichText::new("It has ended. Its graphs are as it left them.")
                                    .color(ui.visuals().warn_fg_color),
                            );
                        }
                    }
                    ui.add_space(12.0);
                    self.graphs(ui, model, now);
                    if let Some(m) = details.and_then(|d| d.memory) {
                        ui.add_space(12.0);
                        memory(ui, &m);
                    }
                    if let Pick::App(pid) = self.pick
                        && let Some(app) = model.app(pid)
                    {
                        ui.add_space(12.0);
                        members(ui, model, app, icons, &mut asked);
                    }
                    if let (Some(d), Some(_)) = (details, proc) {
                        ui.add_space(12.0);
                        lists(ui, d);
                    }
                });
            });
        if custom {
            frame::edges(&ctx, radius, style.border);
        }
        asked
    }

    fn graphs(&self, ui: &mut Ui, model: &Model, now: f64) {
        let v = ui.visuals().clone();
        let accent = v.selection.bg_fill;
        let line = |series, name| Line {
            series,
            colour: accent,
            name,
        };
        let with_gpu = !model.snapshot.gpus.is_empty();
        ui.columns(2, |c| {
            c[0].label(RichText::new("CPU, of the whole machine").small().weak());
            graph::graph(
                &mut c[0],
                &[line(&self.cpu, "")],
                Scale::Share,
                now,
                SPAN,
                GRAPH,
            );
            c[1].label(RichText::new("Memory").small().weak());
            graph::graph(
                &mut c[1],
                &[line(&self.memory, "")],
                Scale::Bytes,
                now,
                SPAN,
                GRAPH,
            );
        });
        ui.add_space(8.0);
        ui.columns(2, |c| {
            c[0].label(RichText::new("Disk, read and written").small().weak());
            graph::graph(
                &mut c[0],
                &[Line {
                    series: &self.disk,
                    colour: second_colour(&v),
                    name: "",
                }],
                Scale::Rate,
                now,
                SPAN,
                GRAPH,
            );
            if with_gpu {
                c[1].label(RichText::new("GPU").small().weak());
                graph::graph(
                    &mut c[1],
                    &[line(&self.gpu, "")],
                    Scale::Share,
                    now,
                    SPAN,
                    GRAPH,
                );
            }
        });
    }
}

/// How long ago `started` (seconds since the epoch) was.
fn ago(started: u64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    duration(now.saturating_sub(started))
}

/// What it is and what it's doing, in two columns.
fn summary(ui: &mut Ui, p: &Proc, d: Option<&Details>) {
    let v = ui.visuals().clone();
    // Waiting on a disk or device, which nothing can interrupt, is where
    // a hung process often is.
    let stuck = d
        .and_then(|d| d.state.as_deref())
        .is_some_and(|s| s.starts_with('D'));
    egui::Grid::new("summary")
        .num_columns(2)
        .spacing(vec2(20.0, 4.0))
        .show(ui, |ui| {
            let row = |ui: &mut Ui, label: &str, value: RichText| {
                ui.label(RichText::new(label).weak());
                ui.add(egui::Label::new(value).wrap());
                ui.end_row();
            };
            if let Some(d) = d {
                let state = d.state.clone().unwrap_or_default();
                let state = match &d.waiting_in {
                    Some(w) => format!("{state}, in {w}"),
                    None => state,
                };
                let state = RichText::new(state).strong();
                row(
                    ui,
                    "State",
                    if stuck {
                        state.color(v.warn_fg_color)
                    } else {
                        state
                    },
                );
            }
            row(ui, "Command", RichText::new(&p.command).monospace());
            if let Some(d) = d {
                if let Some(exe) = &d.exe {
                    row(ui, "Program", RichText::new(exe).monospace());
                }
                if let Some(cwd) = &d.cwd {
                    row(ui, "Folder", RichText::new(cwd).monospace());
                }
                if let Some(cgroup) = &d.cgroup {
                    row(ui, "Control group", RichText::new(cgroup).monospace());
                }
                if let Some((user, system)) = d.cpu_time {
                    row(
                        ui,
                        "CPU time",
                        RichText::new(format!("{user:.1} s its own, {system:.1} s the kernel's")),
                    );
                }
                if let Some((waited, made)) = d.switches {
                    row(
                        ui,
                        "Switches",
                        RichText::new(format!("{waited} to wait, {made} made to")),
                    );
                }
                if let Some(nice) = d.nice {
                    row(ui, "Priority", RichText::new(format!("nice {nice}")));
                }
                if let Some(score) = d.oom_score {
                    row(
                        ui,
                        "Out of memory",
                        RichText::new(format!("score {score}, the highest ended first")),
                    );
                }
                let open = d.files.len() + d.sockets.len() + d.other_fds;
                let limit = d.fd_limit.map_or(String::new(), |l| format!(" of {l}"));
                row(ui, "Open", RichText::new(format!("{open}{limit} files")));
            }
        });
    if stuck {
        ui.label(
            RichText::new(
                "Waiting on a disk or device, which no signal interrupts until it answers.",
            )
            .small()
            .color(v.warn_fg_color),
        );
    }
    if d.is_none() {
        ui.label(RichText::new("Reading its details…").weak());
    }
}

/// Its memory as a bar: its own, files', shared and swapped.
fn memory(ui: &mut Ui, m: &Memory) {
    let v = ui.visuals().clone();
    let parts = [
        ("Its own", m.anonymous, v.selection.bg_fill),
        ("Files", m.file, second_colour(&v)),
        ("Shared", m.shared, v.weak_text_color()),
        ("Swapped out", m.swap, v.warn_fg_color),
    ];
    let total: u64 = parts.iter().map(|p| p.1).sum();
    ui.label(RichText::new("Memory").strong());
    ui.label(
        RichText::new(format!(
            "{} resident; its fair share of what it shares with others counted, {}",
            graph::bytes(m.rss as f64),
            graph::bytes(m.pss as f64)
        ))
        .small()
        .weak(),
    );
    ui.add_space(4.0);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 14.0), Sense::hover());
    ui.painter().rect_filled(rect, 3.0, v.extreme_bg_color);
    let mut x = rect.left();
    for (_, size, colour) in parts {
        if total == 0 || size == 0 {
            continue;
        }
        let w = rect.width() * size as f32 / total as f32;
        ui.painter().rect_filled(
            egui::Rect::from_x_y_ranges(x..=x + w, rect.y_range()),
            0.0,
            colour,
        );
        x += w;
    }
    ui.horizontal_wrapped(|ui| {
        for (name, size, colour) in parts {
            let (r, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
            ui.painter().rect_filled(r.shrink(2.0), 2.0, colour);
            ui.label(RichText::new(format!("{name} {}", graph::bytes(size as f64))).small());
            ui.add_space(8.0);
        }
    });
}

/// An app's processes, each a click from its own window.
fn members(
    ui: &mut Ui,
    model: &Model,
    app: &crate::groups::App,
    icons: &mut Icons,
    asked: &mut Vec<Do>,
) {
    let ctx = ui.ctx().clone();
    ui.label(
        RichText::new(format!(
            "{}'s processes ({})",
            app_name(app, icons, &ctx),
            app.members.len()
        ))
        .strong(),
    );
    egui::Grid::new("members")
        .num_columns(4)
        .spacing(vec2(18.0, 3.0))
        .striped(true)
        .show(ui, |ui| {
            for (pid, depth) in &app.members {
                let Some(p) = model.process(*pid) else {
                    continue;
                };
                ui.horizontal(|ui| {
                    ui.add_space(*depth as f32 * 12.0);
                    if ui.link(&p.name).on_hover_text(&p.command).clicked() {
                        asked.push(Do::Open(Pick::Proc(*pid)));
                    }
                });
                ui.label(RichText::new(pid.to_string()).small().weak());
                ui.label(format!("{:.1}%", p.cpu));
                ui.label(graph::bytes(p.memory as f64));
                ui.end_row();
            }
        });
}

/// Threads, connections, files and environment, the long lists, folded.
fn lists(ui: &mut Ui, d: &Details) {
    let weak = ui.visuals().weak_text_color();
    egui::CollapsingHeader::new(format!("Threads ({})", d.threads.len()))
        .default_open(true)
        .show(ui, |ui| {
            egui::Grid::new("threads")
                .num_columns(4)
                .spacing(vec2(18.0, 2.0))
                .striped(true)
                .show(ui, |ui| {
                    for t in d.threads.iter().take(THREADS) {
                        ui.label(&t.name);
                        ui.label(RichText::new(t.tid.to_string()).small().color(weak));
                        ui.label(format!("{:.1}%", t.cpu));
                        ui.label(RichText::new(thread_state(t.state)).small().color(weak));
                        ui.end_row();
                    }
                });
            if d.threads.len() > THREADS {
                ui.label(
                    RichText::new(format!("and {} more, idle", d.threads.len() - THREADS))
                        .small()
                        .color(weak),
                );
            }
            ui.label(
                RichText::new("CPU here is of one core.")
                    .small()
                    .color(weak),
            );
        });
    let network: Vec<_> = d.sockets.iter().filter(|s| s.kind != "Unix").collect();
    let local: Vec<_> = d.sockets.iter().filter(|s| s.kind == "Unix").collect();
    egui::CollapsingHeader::new(format!("Network connections ({})", network.len()))
        .default_open(!network.is_empty())
        .show(ui, |ui| {
            egui::Grid::new("sockets")
                .num_columns(4)
                .spacing(vec2(18.0, 2.0))
                .striped(true)
                .show(ui, |ui| {
                    for s in &network {
                        ui.label(RichText::new(s.kind).small().color(weak));
                        ui.label(RichText::new(&s.local).monospace());
                        ui.label(RichText::new(&s.remote).monospace());
                        ui.label(RichText::new(&s.state).small().color(weak));
                        ui.end_row();
                    }
                });
        });
    egui::CollapsingHeader::new(format!(
        "Open files ({}, and {} local sockets, {} pipes and others)",
        d.files.len(),
        local.len(),
        d.other_fds
    ))
    .show(ui, |ui| {
        for f in &d.files {
            ui.label(RichText::new(f).monospace().small());
        }
        for s in local.iter().filter(|s| !s.local.is_empty()) {
            ui.label(
                RichText::new(format!("{} (socket)", s.local))
                    .monospace()
                    .small(),
            );
        }
    });
    egui::CollapsingHeader::new(format!("Environment ({})", d.environment.len())).show(ui, |ui| {
        if d.environment.is_empty() {
            ui.label(
                RichText::new("Not readable: it's another user's.")
                    .small()
                    .color(weak),
            );
        }
        for var in &d.environment {
            ui.add(egui::Label::new(RichText::new(var).monospace().small()).wrap());
        }
    });
}

fn thread_state(state: char) -> &'static str {
    match state {
        'R' => "running",
        'S' => "sleeping",
        'D' => "waiting on a device",
        'T' | 't' => "stopped",
        'Z' => "exited",
        'I' => "idle",
        _ => "",
    }
}
