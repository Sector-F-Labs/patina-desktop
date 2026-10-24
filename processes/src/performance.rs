//! The Performance view (ADR-0088): the CPU, memory, disks and each
//! network interface down the side, each with a sparkline of the last
//! minute; the one chosen drawn large, with what it is and what's using it
//! most, each a click from its own window.

use std::collections::BTreeMap;

use egui::{Color32, RichText, Sense, Ui, pos2, vec2};

use crate::graph::{self, Line, Scale, Series};
use crate::groups::Totals;
use crate::icons::Icons;
use crate::processes::{Model, Pick, app_name};

/// What a graph shows: the last minute.
pub const SPAN: f64 = 60.0;
const SIDEBAR_ROW: f32 = 58.0;
const SPARK: egui::Vec2 = vec2(84.0, 38.0);
const CORE: egui::Vec2 = vec2(120.0, 54.0);
/// How many of the busiest are listed.
const TOP: usize = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resource {
    Cpu,
    Memory,
    Disk,
    Network(String),
    /// A GPU, by its place in the list.
    Gpu(usize),
}

/// Everything read so far, as series.
#[derive(Default)]
pub struct History {
    pub cpu: Series,
    pub cores: Vec<Series>,
    pub memory: Series,
    pub swap: Series,
    pub read: Series,
    pub written: Series,
    /// Each interface's receiving and sending.
    pub network: BTreeMap<String, (Series, Series)>,
    pub gpus: Vec<GpuHistory>,
    /// All the interfaces' receiving and sending together.
    pub network_total: (Series, Series),
}

/// A GPU's series: busy, its memory in use, and video encoding and
/// decoding, all 0 to 100.
#[derive(Default)]
pub struct GpuHistory {
    pub busy: Series,
    pub memory: Series,
    pub encoder: Series,
    pub decoder: Series,
}

impl History {
    /// Adds `model`'s reading, at `at` seconds.
    pub fn push(&mut self, at: f64, model: &Model) {
        let s = &model.snapshot;
        self.cpu.push(at, f64::from(s.cpu));
        self.cores.resize_with(s.cores.len(), Series::default);
        for (series, core) in self.cores.iter_mut().zip(&s.cores) {
            series.push(at, f64::from(*core));
        }
        let percent = |used: u64, total: u64| used as f64 / total.max(1) as f64 * 100.0;
        self.memory.push(at, percent(s.memory.used, s.memory.total));
        self.swap
            .push(at, percent(s.memory.swap_used, s.memory.swap_total));
        self.read.push(at, s.disk_read);
        self.written.push(at, s.disk_written);
        self.gpus.resize_with(s.gpus.len(), GpuHistory::default);
        for (h, gpu) in self.gpus.iter_mut().zip(&s.gpus) {
            h.busy.push(at, f64::from(gpu.busy.unwrap_or(0.0)));
            if let (Some(used), Some(total)) = (gpu.memory_used, gpu.memory_total) {
                h.memory.push(at, percent(used, total));
            }
            if let Some(e) = gpu.encoder {
                h.encoder.push(at, f64::from(e));
            }
            if let Some(d) = gpu.decoder {
                h.decoder.push(at, f64::from(d));
            }
        }
        let received: f64 = s.interfaces.iter().map(|i| i.received).sum();
        let sent: f64 = s.interfaces.iter().map(|i| i.sent).sum();
        self.network_total.0.push(at, received);
        self.network_total.1.push(at, sent);
        for i in &s.interfaces {
            let (received, sent) = self.network.entry(i.name.clone()).or_default();
            received.push(at, i.received);
            sent.push(at, i.sent);
        }
    }
}

/// What a click in the view asks for.
pub enum Do {
    /// Open this one's own window.
    Open(Pick),
}

pub struct PerformanceView {
    pub resource: Resource,
}

impl PerformanceView {
    pub fn new() -> Self {
        Self {
            resource: Resource::Cpu,
        }
    }

    /// The resources down the side.
    pub fn sidebar(&mut self, ui: &mut Ui, model: &Model, history: &History, now: f64) {
        let s = &model.snapshot;
        let second = second_colour(ui.visuals());
        let accent = ui.visuals().selection.bg_fill;
        let mut rows: Vec<(Resource, String, String, Vec<Line>, Scale)> = vec![
            (
                Resource::Cpu,
                "CPU".into(),
                format!("{:.0}%", s.cpu),
                vec![line(
                    &history.cpu,
                    cpu_colour(&s.cpu_name, ui.visuals()),
                    "",
                )],
                Scale::Percent,
            ),
            (
                Resource::Memory,
                "Memory".into(),
                format!(
                    "{} / {}",
                    graph::bytes(s.memory.used as f64),
                    graph::bytes(s.memory.total as f64)
                ),
                vec![line(&history.memory, accent, "")],
                Scale::Percent,
            ),
            (
                Resource::Disk,
                "Disk".into(),
                format!(
                    "R {} · W {}",
                    graph::rate(s.disk_read),
                    graph::rate(s.disk_written)
                ),
                vec![
                    line(&history.read, accent, ""),
                    line(&history.written, second, ""),
                ],
                Scale::Rate,
            ),
        ];
        for (index, (gpu, h)) in s.gpus.iter().zip(&history.gpus).enumerate() {
            let busy = format!("{:.0}%", gpu.busy.unwrap_or(0.0));
            let value = match (gpu.memory_used, gpu.memory_total) {
                (Some(used), Some(total)) => format!(
                    "{busy} · {} / {}",
                    graph::bytes(used as f64),
                    graph::bytes(total as f64)
                ),
                _ => busy,
            };
            let title = if s.gpus.len() == 1 {
                "GPU".to_string()
            } else {
                format!("GPU {index}")
            };
            rows.push((
                Resource::Gpu(index),
                title,
                value,
                vec![line(&h.busy, gpu_colour(gpu, ui.visuals()), "")],
                Scale::Percent,
            ));
        }
        for i in &s.interfaces {
            if let Some((received, sent)) = history.network.get(&i.name) {
                rows.push((
                    Resource::Network(i.name.clone()),
                    i.name.clone(),
                    format!("⏷ {} · ⏶ {}", graph::rate(i.received), graph::rate(i.sent)),
                    vec![line(received, accent, ""), line(sent, second, "")],
                    Scale::Rate,
                ));
            }
        }

        ui.add_space(6.0);
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (resource, title, value, lines, scale) in rows {
                let (rect, response) =
                    ui.allocate_exact_size(vec2(ui.available_width(), SIDEBAR_ROW), Sense::click());
                let v = ui.visuals().clone();
                let chosen = self.resource == resource;
                if chosen {
                    ui.painter()
                        .rect_filled(rect, 4.0, v.selection.bg_fill.gamma_multiply(0.35));
                } else if response.hovered() {
                    ui.painter()
                        .rect_filled(rect, 4.0, v.widgets.hovered.weak_bg_fill);
                }
                let spark = egui::Rect::from_min_size(
                    pos2(rect.left() + 8.0, rect.center().y - SPARK.y / 2.0),
                    SPARK,
                );
                graph::sparkline(ui, spark, &lines, scale, now, SPAN);
                let x = spark.right() + 10.0;
                let painter = ui.painter_at(rect);
                painter.text(
                    pos2(x, rect.center().y - 2.0),
                    egui::Align2::LEFT_BOTTOM,
                    title,
                    egui::TextStyle::Body.resolve(ui.style()),
                    v.strong_text_color(),
                );
                painter.text(
                    pos2(x, rect.center().y + 2.0),
                    egui::Align2::LEFT_TOP,
                    value,
                    egui::TextStyle::Small.resolve(ui.style()),
                    v.weak_text_color(),
                );
                if response.clicked() {
                    self.resource = resource;
                }
            }
        });
    }

    pub fn ui(
        &mut self,
        ui: &mut Ui,
        model: &Model,
        history: &History,
        icons: &mut Icons,
        now: f64,
    ) -> Vec<Do> {
        // An interface that went away.
        if let Resource::Network(name) = &self.resource
            && !history.network.contains_key(name)
        {
            self.resource = Resource::Cpu;
        }
        if let Resource::Gpu(index) = self.resource
            && index >= model.snapshot.gpus.len().min(history.gpus.len())
        {
            self.resource = Resource::Cpu;
        }
        let mut asked = Vec::new();
        egui::ScrollArea::vertical().show(ui, |ui| match self.resource.clone() {
            Resource::Cpu => cpu(ui, model, history, icons, now, &mut asked),
            Resource::Memory => memory(ui, model, history, icons, now, &mut asked),
            Resource::Disk => disk(ui, model, history, icons, now, &mut asked),
            Resource::Network(name) => network(ui, model, history, &name, now),
            Resource::Gpu(index) => gpu(
                ui,
                model,
                &history.gpus[index],
                index,
                icons,
                now,
                &mut asked,
            ),
        });
        asked
    }
}

pub fn line<'a>(series: &'a Series, colour: Color32, name: &'a str) -> Line<'a> {
    Line {
        series,
        colour,
        name,
    }
}

/// The makers' colours, which a CPU's and GPU's graphs are drawn in, so
/// what's under the graph shows at a glance: the one place the graphs
/// don't take the skin's accent.
const NVIDIA: Color32 = Color32::from_rgb(0x76, 0xb9, 0x00);
const AMD: Color32 = Color32::from_rgb(0xed, 0x1c, 0x24);
const INTEL: Color32 = Color32::from_rgb(0x00, 0x71, 0xc5);

/// A CPU's colour, by the maker its model names; else the skin's accent.
pub fn cpu_colour(name: &str, v: &egui::Visuals) -> Color32 {
    let name = name.to_lowercase();
    if name.contains("amd") || name.contains("ryzen") || name.contains("epyc") {
        AMD
    } else if name.contains("intel") {
        INTEL
    } else {
        v.selection.bg_fill
    }
}

/// A GPU's colour, by its driver, else the maker its name gives; else the
/// skin's accent.
pub fn gpu_colour(gpu: &crate::gpu::Gpu, v: &egui::Visuals) -> Color32 {
    let name = gpu.name.to_lowercase();
    match gpu.driver.as_str() {
        "nvidia" | "nouveau" | "nova" => NVIDIA,
        "amdgpu" | "radeon" => AMD,
        "i915" | "xe" => INTEL,
        _ if name.contains("nvidia") => NVIDIA,
        _ if name.contains("amd") || name.contains("radeon") => AMD,
        _ if name.contains("intel") => INTEL,
        _ => v.selection.bg_fill,
    }
}

/// The second line's colour, for writing and sending: the skin's link
/// colour, which every skin sets apart from its accent.
pub fn second_colour(v: &egui::Visuals) -> Color32 {
    v.hyperlink_color
}

/// A title, and what it is on the right.
fn heading(ui: &mut Ui, title: &str, detail: &str) {
    ui.horizontal(|ui| {
        ui.heading(title);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new(detail).weak());
        });
    });
    ui.add_space(6.0);
}

fn big_graph(ui: &mut Ui, lines: &[Line], scale: Scale, now: f64) {
    let height = (ui.available_height() * 0.4).clamp(140.0, 260.0);
    graph::graph(ui, lines, scale, now, SPAN, height);
}

/// Facts in two columns, label and value.
fn facts(ui: &mut Ui, id: &str, rows: &[(&str, String)]) {
    egui::Grid::new(id)
        .num_columns(2)
        .spacing(vec2(24.0, 4.0))
        .show(ui, |ui| {
            for (label, value) in rows {
                ui.label(RichText::new(*label).weak());
                ui.label(RichText::new(value).strong());
                ui.end_row();
            }
        });
}

/// The `TOP` busiest by `key`, apps counted whole: name, PID, and the
/// value as `format` writes it.
fn busiest(
    ui: &mut Ui,
    title: &str,
    model: &Model,
    icons: &mut Icons,
    key: impl Fn(Totals) -> f64,
    format: impl Fn(f64) -> String,
    asked: &mut Vec<Do>,
) {
    let ctx = ui.ctx().clone();
    let mut all: Vec<(String, Pick, f64)> = model
        .apps
        .iter()
        .map(|a| (app_name(a, icons, &ctx), Pick::App(a.pid), key(a.totals)))
        .chain(
            model
                .background()
                .map(|p| (p.name.clone(), Pick::Proc(p.pid), key(Totals::of(p)))),
        )
        .filter(|(.., value)| *value > 0.0)
        .collect();
    all.sort_by(|a, b| b.2.total_cmp(&a.2));
    ui.add_space(12.0);
    ui.label(RichText::new(title).strong());
    if all.is_empty() {
        ui.label(RichText::new("Nothing").weak());
        return;
    }
    egui::Grid::new(title)
        .num_columns(2)
        .spacing(vec2(24.0, 2.0))
        .show(ui, |ui| {
            for (name, pick, value) in all.into_iter().take(TOP) {
                let (Pick::App(pid) | Pick::Proc(pid)) = pick;
                if ui
                    .link(&name)
                    .on_hover_text(format!("PID {pid}: open its window"))
                    .clicked()
                {
                    asked.push(Do::Open(pick));
                }
                ui.label(format(value));
                ui.end_row();
            }
        });
}

pub fn duration(seconds: u64) -> String {
    let (d, h, m) = (seconds / 86400, seconds % 86400 / 3600, seconds % 3600 / 60);
    match (d, h) {
        (0, 0) => format!("{m} min"),
        (0, _) => format!("{h} h {m} min"),
        _ => format!("{d} d {h} h"),
    }
}

fn cpu(
    ui: &mut Ui,
    model: &Model,
    history: &History,
    icons: &mut Icons,
    now: f64,
    asked: &mut Vec<Do>,
) {
    let s = &model.snapshot;
    let accent = cpu_colour(&s.cpu_name, ui.visuals());
    heading(ui, "CPU", &s.cpu_name);
    big_graph(ui, &[line(&history.cpu, accent, "")], Scale::Percent, now);
    ui.add_space(8.0);
    // Each CPU, small.
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
        for (i, series) in history.cores.iter().enumerate() {
            let (rect, _) = ui.allocate_exact_size(CORE, Sense::hover());
            let plot = egui::Rect::from_min_max(rect.min + vec2(0.0, 14.0), rect.max);
            ui.painter().text(
                rect.left_top(),
                egui::Align2::LEFT_TOP,
                format!("CPU {i} · {:.0}%", series.last().unwrap_or(0.0)),
                egui::TextStyle::Small.resolve(ui.style()),
                ui.visuals().weak_text_color(),
            );
            graph::sparkline(
                ui,
                plot,
                &[line(series, accent, "")],
                Scale::Percent,
                now,
                SPAN,
            );
        }
    });
    ui.add_space(12.0);
    let threads = s.cores.len();
    let cores = match s.physical_cores {
        Some(c) if c != threads => format!("{c} ({threads} threads)"),
        _ => threads.to_string(),
    };
    let mut rows = vec![("In use", format!("{:.0}%", s.cpu)), ("Cores", cores)];
    if s.frequency > 0 {
        rows.push(("Speed", format!("{:.2} GHz", s.frequency as f64 / 1000.0)));
    }
    rows.push((
        "Load",
        format!("{:.2} · {:.2} · {:.2}", s.load[0], s.load[1], s.load[2]),
    ));
    rows.push(("Processes", s.processes.len().to_string()));
    rows.push(("Up", duration(s.uptime)));
    facts(ui, "cpu facts", &rows);
    busiest(
        ui,
        "Using the most CPU",
        model,
        icons,
        |t| f64::from(t.cpu),
        |v| format!("{v:.1}%"),
        asked,
    );
}

fn memory(
    ui: &mut Ui,
    model: &Model,
    history: &History,
    icons: &mut Icons,
    now: f64,
    asked: &mut Vec<Do>,
) {
    let m = model.snapshot.memory;
    let v = ui.visuals().clone();
    heading(
        ui,
        "Memory",
        &format!("{} in all", graph::bytes(m.total as f64)),
    );
    let mut lines = vec![line(&history.memory, v.selection.bg_fill, "Memory")];
    if m.swap_total > 0 {
        lines.push(line(&history.swap, second_colour(&v), "Swap"));
    }
    big_graph(ui, &lines, Scale::Percent, now);
    ui.add_space(8.0);
    let percent = |used: u64, total: u64| used as f64 / total.max(1) as f64 * 100.0;
    let mut rows = vec![
        (
            "In use",
            format!(
                "{} ({:.0}%)",
                graph::bytes(m.used as f64),
                percent(m.used, m.total)
            ),
        ),
        ("Available", graph::bytes(m.available as f64)),
    ];
    rows.push((
        "Swap",
        if m.swap_total == 0 {
            "None".to_string()
        } else {
            format!(
                "{} of {}",
                graph::bytes(m.swap_used as f64),
                graph::bytes(m.swap_total as f64)
            )
        },
    ));
    facts(ui, "memory facts", &rows);
    busiest(
        ui,
        "Using the most memory",
        model,
        icons,
        |t| t.memory as f64,
        graph::bytes,
        asked,
    );
}

fn disk(
    ui: &mut Ui,
    model: &Model,
    history: &History,
    icons: &mut Icons,
    now: f64,
    asked: &mut Vec<Do>,
) {
    let s = &model.snapshot;
    let v = ui.visuals().clone();
    heading(ui, "Disk", "Reading and writing, all disks");
    big_graph(
        ui,
        &[
            line(&history.read, v.selection.bg_fill, "Read"),
            line(&history.written, second_colour(&v), "Write"),
        ],
        Scale::Rate,
        now,
    );
    legend(
        ui,
        &[("Read", v.selection.bg_fill), ("Write", second_colour(&v))],
    );
    ui.add_space(8.0);
    facts(
        ui,
        "disk facts",
        &[
            ("Reading", graph::rate(s.disk_read)),
            ("Writing", graph::rate(s.disk_written)),
        ],
    );
    ui.add_space(12.0);
    ui.label(RichText::new("Space").strong());
    egui::Grid::new("volumes")
        .num_columns(3)
        .spacing(vec2(16.0, 4.0))
        .show(ui, |ui| {
            for vol in &s.volumes {
                if vol.total == 0 {
                    continue;
                }
                let used = vol.total - vol.available.min(vol.total);
                ui.label(&vol.mount)
                    .on_hover_text(format!("{} ({})", vol.device, vol.file_system));
                ui.add(
                    egui::ProgressBar::new(used as f32 / vol.total as f32)
                        .desired_width(160.0)
                        .desired_height(10.0),
                );
                ui.label(
                    RichText::new(format!(
                        "{} free of {}",
                        graph::bytes(vol.available as f64),
                        graph::bytes(vol.total as f64)
                    ))
                    .weak(),
                );
                ui.end_row();
            }
        });
    busiest(
        ui,
        "Using the disks most",
        model,
        icons,
        |t| t.disk,
        graph::rate,
        asked,
    );
}

fn gpu(
    ui: &mut Ui,
    model: &Model,
    history: &GpuHistory,
    index: usize,
    icons: &mut Icons,
    now: f64,
    asked: &mut Vec<Do>,
) {
    let g = &model.snapshot.gpus[index];
    let v = ui.visuals().clone();
    heading(ui, "GPU", &g.name);
    let colour = gpu_colour(g, &v);
    let mut lines = vec![line(&history.busy, colour, "Busy")];
    if g.memory_total.is_some() {
        lines.push(line(&history.memory, second_colour(&v), "Memory"));
    }
    big_graph(ui, &lines, Scale::Percent, now);
    if g.memory_total.is_some() {
        legend(
            ui,
            &[("Busy", colour), ("Memory in use", second_colour(&v))],
        );
    }
    // Video encoding and decoding, small, where the driver counts them.
    if g.encoder.is_some() || g.decoder.is_some() {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
            for (name, series) in [
                ("Video encode", &history.encoder),
                ("Video decode", &history.decoder),
            ] {
                let (rect, _) = ui.allocate_exact_size(CORE * vec2(1.5, 1.0), Sense::hover());
                let plot = egui::Rect::from_min_max(rect.min + vec2(0.0, 14.0), rect.max);
                ui.painter().text(
                    rect.left_top(),
                    egui::Align2::LEFT_TOP,
                    format!("{name} · {:.0}%", series.last().unwrap_or(0.0)),
                    egui::TextStyle::Small.resolve(ui.style()),
                    v.weak_text_color(),
                );
                graph::sparkline(
                    ui,
                    plot,
                    &[line(series, colour, "")],
                    Scale::Percent,
                    now,
                    SPAN,
                );
            }
        });
    }
    ui.add_space(12.0);
    let mut rows = vec![("Busy", format!("{:.0}%", g.busy.unwrap_or(0.0)))];
    if let (Some(used), Some(total)) = (g.memory_used, g.memory_total) {
        rows.push((
            "Memory",
            format!(
                "{} of {}",
                graph::bytes(used as f64),
                graph::bytes(total as f64)
            ),
        ));
    }
    if let Some(t) = g.temperature {
        rows.push(("Temperature", format!("{t} °C")));
    }
    match (g.power, g.power_limit) {
        (Some(p), Some(limit)) => rows.push(("Power", format!("{p:.1} W of {limit:.0} W"))),
        (Some(p), None) => rows.push(("Power", format!("{p:.1} W"))),
        _ => {}
    }
    if let Some(c) = g.clock {
        rows.push(("Clock", format!("{c} MHz")));
    }
    if let Some(c) = g.memory_clock {
        rows.push(("Memory clock", format!("{c} MHz")));
    }
    if let Some(f) = g.fan {
        rows.push(("Fan", format!("{f}%")));
    }
    rows.push(("Driver", g.driver.clone()));
    rows.push(("Address", g.address.clone()));
    facts(ui, "gpu facts", &rows);
    busiest(
        ui,
        "Using the GPU most",
        model,
        icons,
        |t| f64::from(t.gpu),
        |v| format!("{v:.0}%"),
        asked,
    );
    busiest(
        ui,
        "Holding the most GPU memory",
        model,
        icons,
        |t| t.gpu_memory as f64,
        graph::bytes,
        asked,
    );
}

fn network(ui: &mut Ui, model: &Model, history: &History, name: &str, now: f64) {
    let Some(interface) = model.snapshot.interfaces.iter().find(|i| i.name == name) else {
        return;
    };
    let Some((received, sent)) = history.network.get(name) else {
        return;
    };
    let v = ui.visuals().clone();
    heading(ui, name, "Network");
    big_graph(
        ui,
        &[
            line(received, v.selection.bg_fill, "⏷"),
            line(sent, second_colour(&v), "⏶"),
        ],
        Scale::Rate,
        now,
    );
    legend(
        ui,
        &[
            ("Receiving", v.selection.bg_fill),
            ("Sending", second_colour(&v)),
        ],
    );
    ui.add_space(8.0);
    let mut rows = vec![
        ("Receiving", graph::rate(interface.received)),
        ("Sending", graph::rate(interface.sent)),
        ("Received", graph::bytes(interface.total_received as f64)),
        ("Sent", graph::bytes(interface.total_sent as f64)),
    ];
    for address in &interface.addresses {
        rows.push(("Address", address.clone()));
    }
    facts(ui, "network facts", &rows);
}

/// Which colour is which line.
fn legend(ui: &mut Ui, items: &[(&str, Color32)]) {
    ui.horizontal(|ui| {
        for (name, colour) in items {
            let (rect, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
            ui.painter().rect_filled(rect.shrink(2.0), 2.0, *colour);
            ui.label(RichText::new(*name).small());
            ui.add_space(8.0);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graphs_wear_their_makers_colours() {
        let v = egui::Visuals::dark();
        assert_eq!(cpu_colour("AMD Ryzen 5 3600X 6-Core Processor", &v), AMD);
        assert_eq!(cpu_colour("Intel(R) Core(TM) i7-1165G7", &v), INTEL);
        assert_eq!(cpu_colour("Cortex-A72", &v), v.selection.bg_fill);
        let gpu = |driver: &str, name: &str| crate::gpu::Gpu {
            driver: driver.into(),
            name: name.into(),
            ..Default::default()
        };
        assert_eq!(gpu_colour(&gpu("nvidia", "NVIDIA RTX 2000"), &v), NVIDIA);
        assert_eq!(gpu_colour(&gpu("amdgpu", "AMD Navi 21"), &v), AMD);
        assert_eq!(gpu_colour(&gpu("xe", "Intel Arc"), &v), INTEL);
        assert_eq!(gpu_colour(&gpu("vfio-pci", "NVIDIA GA102"), &v), NVIDIA);
        assert_eq!(
            gpu_colour(&gpu("panfrost", "Mali"), &v),
            v.selection.bg_fill
        );
    }

    #[test]
    fn durations_read_in_the_two_biggest_units() {
        assert_eq!(duration(59), "0 min");
        assert_eq!(duration(3 * 3600 + 120), "3 h 2 min");
        assert_eq!(duration(2 * 86400 + 5 * 3600), "2 d 5 h");
    }
}
