//! The Overview, where the process manager opens: the machine in a line, a
//! tile for each of its resources with the last minute's graph (a click
//! opens it in Performance), what looks wrong (the last day's, then the
//! older issues folded), then the busiest on the CPU, the GPU and
//! memory, each a click from its own window.

use std::time::{SystemTime, UNIX_EPOCH};

use egui::{Align2, Rect, RichText, Sense, Ui, pos2, vec2};

use crate::graph::{self, Line, Scale};
use crate::groups::Totals;
use crate::icons::Icons;
use crate::performance::{
    History, Resource, SPAN, cpu_colour, duration, gpu_colour, line, second_colour,
};
use crate::problems::{Go, Problem, Severity};
use crate::processes::{Do as ProcessDo, Model, Pick, app_name};
use crate::ranking::Ranking;

/// A tile's least width, and its height.
const TILE_WIDTH: f32 = 230.0;
const TILE_HEIGHT: f32 = 156.0;
const GAP: f32 = 12.0;
/// How many each list has, and their rows' height.
const BUSIEST: usize = 10;
const ROW: f32 = 26.0;

/// What a click in the Overview asks for.
pub enum Do {
    /// Open this in Performance.
    Show(Resource),
    /// What its right click asked, as in Processes.
    Process(ProcessDo),
    /// Where a problem leads.
    Go(Go),
}

/// A tile: a resource, how much of it is used, and its graph.
struct Tile<'a> {
    resource: Resource,
    title: String,
    value: String,
    detail: String,
    lines: Vec<Line<'a>>,
    scale: Scale,
}

/// The busiest lists' rows, kept steady between readings (`ranking.rs`).
pub struct Busiest {
    cpu: Ranking<Pick>,
    gpu: Ranking<Pick>,
    memory: Ranking<Pick>,
}

impl Busiest {
    pub fn new() -> Self {
        Self {
            cpu: Ranking::new(BUSIEST),
            gpu: Ranking::new(BUSIEST),
            memory: Ranking::new(BUSIEST),
        }
    }

    /// Takes a reading: apps counted whole, and the background processes.
    pub fn update(&mut self, model: &Model) {
        let all: Vec<(Pick, Totals)> = model
            .apps
            .iter()
            .map(|a| (Pick::App(a.pid), a.totals))
            .chain(
                model
                    .background()
                    .map(|p| (Pick::Proc(p.pid), Totals::of(p))),
            )
            .collect();
        for (ranking, measure) in [
            (&mut self.cpu, Measure::Cpu),
            (&mut self.gpu, Measure::Gpu),
            (&mut self.memory, Measure::Memory),
        ] {
            ranking.update(all.iter().map(|(pick, t)| (*pick, measure.of(*t))));
        }
    }

    fn order(&self, measure: Measure) -> &[Pick] {
        match measure {
            Measure::Cpu => self.cpu.order(),
            Measure::Gpu => self.gpu.order(),
            Measure::Memory => self.memory.order(),
        }
    }
}

pub fn ui(
    ui: &mut Ui,
    model: &Model,
    history: &History,
    busiest: &Busiest,
    problems: &[Problem],
    icons: &mut Icons,
    now: f64,
) -> Vec<Do> {
    let mut asked = Vec::new();
    egui::ScrollArea::vertical().show(ui, |ui| {
        machine(ui, model);
        ui.add_space(12.0);
        tiles(ui, model, history, now, &mut asked);
        ui.add_space(16.0);
        show_problems(ui, problems, &mut asked);
        ui.add_space(20.0);
        lists(ui, model, busiest, icons, &mut asked);
    });
    asked
}

/// How many problems are listed; the rest are in the log.
const PROBLEMS: usize = 6;

/// What looks wrong, worst and newest first, each a click from where it
/// can be looked into: what's going on or happened in the last day, then,
/// folded, the older issues.
fn show_problems(ui: &mut Ui, problems: &[Problem], asked: &mut Vec<Do>) {
    if problems.is_empty() {
        ui.label(
            RichText::new(
                "Nothing looks wrong: nothing waiting, and no crashes or kernel errors since \
                 the machine started.",
            )
            .weak(),
        );
        return;
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64());
    let (recent, older): (Vec<&Problem>, Vec<&Problem>) =
        problems.iter().partition(|p| p.recent(now));
    ui.label(RichText::new("Problems").strong());
    ui.scope(|ui| {
        ui.set_max_width(crate::log_view::EXPLAINED);
        ui.label(
            RichText::new(
                "Found in the kernel's log since the machine started (the Log tab), in how much \
             programs waited for the CPU, memory and disks over the last minute (Pressure, \
             in Performance), and in the list of processes. Hover over one for what it means; \
             a click shows where it came from.",
            )
            .small()
            .weak(),
        );
    });
    ui.add_space(4.0);
    if recent.is_empty() {
        ui.label(
            RichText::new(
                "Nothing in the last day: nothing waiting, and no crashes or kernel errors.",
            )
            .weak(),
        );
    }
    for p in recent.iter().take(PROBLEMS) {
        problem(ui, p, asked);
    }
    if recent.len() > PROBLEMS {
        ui.label(
            RichText::new(format!(
                "and {} more in the Log tab",
                recent.len() - PROBLEMS
            ))
            .small()
            .weak(),
        );
    }
    if !older.is_empty() {
        egui::CollapsingHeader::new(format!("Older issues ({})", older.len()))
            .id_salt("older issues")
            .default_open(false)
            .show(ui, |ui| {
                for p in older {
                    problem(ui, p, asked);
                }
            });
    }
}

/// A problem: what, and when or how much; a click leads to it.
fn problem(ui: &mut Ui, p: &Problem, asked: &mut Vec<Do>) {
    let v = ui.visuals().clone();
    let body = egui::TextStyle::Body.resolve(ui.style());
    let small = egui::TextStyle::Small.resolve(ui.style());
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::click());
    let clickable = p.go != Go::Nowhere;
    if clickable && response.hovered() {
        ui.painter()
            .rect_filled(rect, 4.0, v.widgets.hovered.weak_bg_fill);
    }
    let colour = match p.severity {
        Severity::Error => v.error_fg_color,
        Severity::Warning => v.warn_fg_color,
    };
    let painter = ui.painter_at(rect);
    painter.circle_filled(pos2(rect.left() + 10.0, rect.top() + 12.0), 4.0, colour);
    painter.text(
        pos2(rect.left() + 22.0, rect.top() + 3.0),
        Align2::LEFT_TOP,
        &p.title,
        body,
        v.strong_text_color(),
    );
    painter.text(
        pos2(rect.left() + 22.0, rect.top() + 22.0),
        Align2::LEFT_TOP,
        &p.detail,
        small,
        v.weak_text_color(),
    );
    let response = response.on_hover_ui(|ui| {
        ui.set_max_width(420.0);
        ui.label(p.meaning);
    });
    if clickable
        && response
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
    {
        asked.push(Do::Go(p.go.clone()));
    }
}

/// The machine's name, system, kernel and uptime.
fn machine(ui: &mut Ui, model: &Model) {
    let s = &model.snapshot;
    ui.horizontal_wrapped(|ui| {
        if !s.host.is_empty() {
            ui.heading(&s.host);
        }
        let mut about: Vec<String> = Vec::new();
        if !s.os.is_empty() {
            about.push(s.os.clone());
        }
        if !s.kernel.is_empty() {
            about.push(format!("kernel {}", s.kernel));
        }
        about.push(format!("up {}", duration(s.uptime)));
        ui.label(RichText::new(about.join(" · ")).weak());
    });
}

fn tiles(ui: &mut Ui, model: &Model, history: &History, now: f64, asked: &mut Vec<Do>) {
    let s = &model.snapshot;
    let v = ui.visuals().clone();
    let accent = v.selection.bg_fill;
    let second = second_colour(&v);
    let percent = |used: u64, total: u64| used as f64 / total.max(1) as f64 * 100.0;

    let mut tiles = vec![
        Tile {
            resource: Resource::Cpu,
            title: "CPU".into(),
            value: format!("{:.0}%", s.cpu),
            detail: format!("{} cores · load {:.2}", s.cores.len(), s.load[0]),
            lines: vec![line(&history.cpu, cpu_colour(&s.cpu_name, &v), "")],
            scale: Scale::Percent,
        },
        Tile {
            resource: Resource::Memory,
            title: "Memory".into(),
            value: format!("{:.0}%", percent(s.memory.used, s.memory.total)),
            detail: format!(
                "{} of {}",
                graph::bytes(s.memory.used as f64),
                graph::bytes(s.memory.total as f64)
            ),
            lines: vec![line(&history.memory, accent, "")],
            scale: Scale::Percent,
        },
    ];
    for (index, (gpu, h)) in s.gpus.iter().zip(&history.gpus).enumerate() {
        let detail = match (gpu.memory_used, gpu.memory_total) {
            (Some(used), Some(total)) => format!(
                "{} of {}",
                graph::bytes(used as f64),
                graph::bytes(total as f64)
            ),
            _ => gpu.name.clone(),
        };
        let detail = match gpu.temperature {
            Some(t) => format!("{detail} · {t} °C"),
            None => detail,
        };
        tiles.push(Tile {
            resource: Resource::Gpu(index),
            title: if s.gpus.len() == 1 {
                "GPU".into()
            } else {
                format!("GPU {index}")
            },
            value: format!("{:.0}%", gpu.busy.unwrap_or(0.0)),
            detail,
            lines: vec![line(&h.busy, gpu_colour(gpu, &v), "")],
            scale: Scale::Percent,
        });
    }
    tiles.push(Tile {
        resource: Resource::Disk,
        title: "Disk".into(),
        value: graph::rate(s.disk_read + s.disk_written),
        detail: format!(
            "Read {} · Write {}",
            graph::rate(s.disk_read),
            graph::rate(s.disk_written)
        ),
        lines: vec![
            line(&history.read, accent, ""),
            line(&history.written, second, ""),
        ],
        scale: Scale::Rate,
    });
    if let Some(first) = s.interfaces.first() {
        let received: f64 = s.interfaces.iter().map(|i| i.received).sum();
        let sent: f64 = s.interfaces.iter().map(|i| i.sent).sum();
        tiles.push(Tile {
            resource: Resource::Network(first.name.clone()),
            title: "Network".into(),
            value: graph::rate(received + sent),
            detail: format!("⏷ {} · ⏶ {}", graph::rate(received), graph::rate(sent)),
            lines: vec![
                line(&history.network_total.0, accent, ""),
                line(&history.network_total.1, second, ""),
            ],
            scale: Scale::Rate,
        });
    }

    // As many across as fit, sharing the width.
    let width = ui.available_width();
    let across = (((width + GAP) / (TILE_WIDTH + GAP)).floor() as usize).clamp(1, tiles.len());
    let tile_width = (width - GAP * (across - 1) as f32) / across as f32;
    for row in tiles.chunks(across) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            for t in row {
                if tile(ui, t, vec2(tile_width, TILE_HEIGHT), now) {
                    asked.push(Do::Show(t.resource.clone()));
                }
            }
        });
        ui.add_space(GAP);
    }
}

/// Draws `t`; whether it was clicked.
fn tile(ui: &mut Ui, t: &Tile, size: egui::Vec2, now: f64) -> bool {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let v = ui.visuals().clone();
    let stroke = if response.hovered() {
        v.widgets.hovered.bg_stroke
    } else {
        v.widgets.noninteractive.bg_stroke
    };
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 6.0, v.faint_bg_color);
    painter.rect_stroke(rect, 6.0, stroke, egui::StrokeKind::Inside);
    let inner = rect.shrink(12.0);
    painter.text(
        inner.left_top(),
        Align2::LEFT_TOP,
        &t.title,
        egui::TextStyle::Body.resolve(ui.style()),
        v.weak_text_color(),
    );
    let value = painter.text(
        inner.left_top() + vec2(0.0, 18.0),
        Align2::LEFT_TOP,
        &t.value,
        egui::FontId::proportional(24.0),
        v.strong_text_color(),
    );
    painter.text(
        pos2(inner.left(), value.bottom() + 2.0),
        Align2::LEFT_TOP,
        &t.detail,
        egui::TextStyle::Small.resolve(ui.style()),
        v.weak_text_color(),
    );
    let plot = Rect::from_min_max(pos2(inner.left(), inner.bottom() - 56.0), inner.max);
    graph::sparkline(ui, plot, &t.lines, t.scale, now, SPAN);
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

/// What a list is of.
#[derive(Clone, Copy)]
enum Measure {
    Cpu,
    Gpu,
    Memory,
}

impl Measure {
    fn title(self) -> &'static str {
        match self {
            Measure::Cpu => "Busiest on the CPU",
            Measure::Gpu => "Busiest on the GPU",
            Measure::Memory => "Most memory",
        }
    }

    fn of(self, t: Totals) -> f64 {
        match self {
            Measure::Cpu => f64::from(t.cpu),
            Measure::Gpu => f64::from(t.gpu),
            Measure::Memory => t.memory as f64,
        }
    }

    fn format(self, t: Totals) -> String {
        match self {
            Measure::Cpu => format!("{:.1}%", t.cpu),
            Measure::Gpu if t.gpu_memory > 0 => {
                format!("{:.0}% · {}", t.gpu, graph::bytes(t.gpu_memory as f64))
            }
            Measure::Gpu => format!("{:.0}%", t.gpu),
            Measure::Memory => graph::bytes(t.memory as f64),
        }
    }
}

/// An app, counted whole, or a background process.
struct Entry {
    pick: Pick,
    name: String,
    icon: Option<egui::TextureHandle>,
    totals: Totals,
}

/// The busiest on the CPU, the GPU (where there is one) and memory, side
/// by side where there's room.
fn lists(ui: &mut Ui, model: &Model, busiest: &Busiest, icons: &mut Icons, asked: &mut Vec<Do>) {
    let ctx = ui.ctx().clone();
    let entries: Vec<Entry> = model
        .apps
        .iter()
        .map(|a| Entry {
            pick: Pick::App(a.pid),
            name: app_name(a, icons, &ctx),
            icon: icons.get(&ctx, &a.app_id).icon.clone(),
            totals: a.totals,
        })
        .chain(model.background().map(|p| Entry {
            pick: Pick::Proc(p.pid),
            name: p.name.clone(),
            icon: None,
            totals: Totals::of(p),
        }))
        .collect();
    let mut measures = vec![Measure::Cpu];
    if !model.snapshot.gpus.is_empty() {
        measures.push(Measure::Gpu);
    }
    measures.push(Measure::Memory);
    let wide = ui.available_width() >= measures.len() as f32 * 300.0;
    if wide {
        ui.columns(measures.len(), |columns| {
            for (ui, measure) in columns.iter_mut().zip(&measures) {
                list(
                    ui,
                    model,
                    &entries,
                    busiest.order(*measure),
                    *measure,
                    asked,
                );
            }
        });
    } else {
        for measure in measures {
            list(ui, model, &entries, busiest.order(measure), measure, asked);
            ui.add_space(16.0);
        }
    }
}

/// The `BUSIEST` heaviest by `measure`, in `order`'s steady rows. A click
/// opens its own window, and a right click has Processes' menu: ending,
/// killing and pausing it from here.
fn list(
    ui: &mut Ui,
    model: &Model,
    entries: &[Entry],
    order: &[Pick],
    measure: Measure,
    asked: &mut Vec<Do>,
) {
    let top: Vec<&Entry> = order
        .iter()
        .filter_map(|pick| entries.iter().find(|e| e.pick == *pick))
        .collect();
    ui.label(RichText::new(measure.title()).strong());
    ui.add_space(4.0);
    if top.is_empty() {
        ui.label(RichText::new("Nothing").weak());
        return;
    }
    let v = ui.visuals().clone();
    let body = egui::TextStyle::Body.resolve(ui.style());
    for e in top {
        let (rect, response) =
            ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
        if response.hovered() || response.context_menu_opened() {
            ui.painter()
                .rect_filled(rect, 2.0, v.widgets.hovered.weak_bg_fill);
        }
        let mut x = rect.left() + 4.0;
        if let Some(icon) = &e.icon {
            let at = Rect::from_min_size(pos2(x, rect.center().y - 8.0), vec2(16.0, 16.0));
            egui::Image::from_texture(icon).paint_at(ui, at);
        }
        x += 22.0;
        let value = ui.painter().text(
            pos2(rect.right() - 6.0, rect.center().y),
            Align2::RIGHT_CENTER,
            measure.format(e.totals),
            body.clone(),
            v.text_color(),
        );
        // The name, cut short of the value.
        let room = (value.left() - 8.0 - x).max(0.0);
        let name = egui::WidgetText::from(e.name.as_str()).into_galley(
            ui,
            Some(egui::TextWrapMode::Truncate),
            room,
            egui::TextStyle::Body,
        );
        ui.painter().galley(
            pos2(x, rect.center().y - name.size().y / 2.0),
            name,
            v.strong_text_color(),
        );
        let response = response.on_hover_text(format!("PID {}: open its window", pid(e.pick)));
        if response.clicked() {
            asked.push(Do::Process(ProcessDo::Open(e.pick)));
        }
        response.context_menu(|ui| {
            for d in crate::processes::menu(ui, model, e.pick, false) {
                asked.push(Do::Process(d));
            }
        });
    }
}

fn pid(pick: Pick) -> u32 {
    match pick {
        Pick::App(p) | Pick::Proc(p) => p,
    }
}
