//! The Processes view (ADR-0088): the apps, each the process with windows
//! and what it started, then the background processes; sortable columns
//! tinted by how much each uses, a filter, and ending, killing, pausing and
//! switching to them from a right click, the Process menu or the keyboard.

use std::collections::{HashMap, HashSet};

use egui::{Align2, Color32, Rect, Sense, Ui, pos2, vec2};

use crate::groups::{self, App, Column, Sort, Totals};
use crate::icons::Icons;
use crate::sample::{Proc, Signal, Snapshot, State};

const ROW: f32 = 24.0;
const ICON: f32 = 16.0;
const INDENT: f32 = 16.0;
/// Disk use this fast is tinted fully.
const DISK_FULL: f64 = 64.0 * 1024.0 * 1024.0;

/// The columns' widths, Name taking what's left.
fn width(column: Column) -> f32 {
    match column {
        Column::Name => 0.0,
        Column::Pid => 64.0,
        Column::User => 96.0,
        Column::Cpu => 64.0,
        Column::Memory => 88.0,
        Column::Disk => 88.0,
        Column::Gpu => 64.0,
    }
}

/// What's chosen in the list: an app's row, or a process's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Pick {
    App(u32),
    Proc(u32),
}

/// What a row asks for, done after the list is drawn.
#[derive(Debug, Clone, PartialEq)]
pub enum Do {
    Pick(Pick),
    /// Open its own window (`window.rs`).
    Open(Pick),
    Toggle(u32),
    Switch(u32),
    Signal(Pick, Signal),
    Copy(String),
    Sort(Column),
}

/// A line of the list.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Line {
    Heading(&'static str, usize),
    App(usize),
    Proc { pid: u32, depth: usize },
}

impl Line {
    fn pick(self, apps: &[App]) -> Option<Pick> {
        match self {
            Line::Heading(..) => None,
            Line::App(i) => Some(Pick::App(apps[i].pid)),
            Line::Proc { pid, .. } => Some(Pick::Proc(pid)),
        }
    }
}

/// The latest reading, worked out for listing.
pub struct Model {
    pub snapshot: Snapshot,
    pub apps: Vec<App>,
    /// Processes in an app.
    grouped: HashSet<u32>,
    by_pid: HashMap<u32, usize>,
    /// This process manager and what it runs under, up to the compositor:
    /// ending one of them ends more than it seems to.
    pub ours: HashSet<u32>,
}

impl Model {
    pub fn new(snapshot: Snapshot) -> Self {
        let windows = snapshot.windows.as_deref().unwrap_or(&[]);
        let (apps, grouped) = groups::apps(&snapshot.processes, windows);
        let by_pid: HashMap<u32, usize> = snapshot
            .processes
            .iter()
            .enumerate()
            .map(|(i, p)| (p.pid, i))
            .collect();
        let mut ours = HashSet::new();
        let mut pid = Some(std::process::id());
        while let Some(p) = pid.and_then(|p| by_pid.get(&p).map(|&i| &snapshot.processes[i])) {
            if p.pid <= 1 || !ours.insert(p.pid) {
                break;
            }
            pid = p.parent;
        }
        Self {
            snapshot,
            apps,
            grouped,
            by_pid,
            ours,
        }
    }

    pub fn process(&self, pid: u32) -> Option<&Proc> {
        self.by_pid.get(&pid).map(|&i| &self.snapshot.processes[i])
    }

    /// The app whose own process is `pid`.
    pub fn app(&self, pid: u32) -> Option<&App> {
        self.apps.iter().find(|a| a.pid == pid)
    }

    /// The processes not in an app.
    pub fn background(&self) -> impl Iterator<Item = &Proc> {
        self.snapshot
            .processes
            .iter()
            .filter(|p| !self.grouped.contains(&p.pid))
    }

    /// What a pick names, for messages.
    pub fn describe(&self, pick: Pick, icons: &mut Icons, ctx: &egui::Context) -> String {
        match pick {
            Pick::App(pid) => match self.app(pid) {
                Some(app) => app_name(app, icons, ctx),
                None => format!("process {pid}"),
            },
            Pick::Proc(pid) => match self.process(pid) {
                Some(p) => format!("{} ({pid})", p.name),
                None => format!("process {pid}"),
            },
        }
    }

    /// The processes `signal` goes to for `pick`: an app's own process to
    /// end it, as closing it would, or all of them otherwise.
    pub fn targets(&self, pick: Pick, signal: Signal) -> Vec<u32> {
        match pick {
            Pick::Proc(pid) => vec![pid],
            Pick::App(pid) => match (signal, self.app(pid)) {
                (Signal::End, _) | (_, None) => vec![pid],
                (_, Some(app)) => app.members.iter().map(|(p, _)| *p).collect(),
            },
        }
    }

    /// Whether `pick` is paused: an app when its own process is.
    pub fn paused(&self, pick: Pick) -> bool {
        let pid = match pick {
            Pick::App(p) | Pick::Proc(p) => p,
        };
        self.process(pid).is_some_and(|p| p.state == State::Stopped)
    }
}

/// An app's name: its entry's, else its app id.
pub fn app_name(app: &App, icons: &mut Icons, ctx: &egui::Context) -> String {
    icons
        .get(ctx, &app.app_id)
        .name
        .clone()
        .unwrap_or_else(|| app.app_id.clone())
}

pub struct ProcessesView {
    pub filter: String,
    pub sort: Sort,
    pub picked: Option<Pick>,
    expanded: HashSet<u32>,
    pub show_kernel: bool,
    pub only_mine: bool,
    /// Asked for from the menu or Ctrl+F.
    pub focus_filter: bool,
    /// The line to bring into view, after the keyboard moved to it.
    scroll_to: Option<usize>,
    /// The list's scroll offset and height last frame.
    scrolled: (f32, f32),
    me: u32,
}

impl ProcessesView {
    pub fn new() -> Self {
        Self {
            filter: String::new(),
            sort: Sort::default(),
            picked: None,
            expanded: HashSet::new(),
            show_kernel: false,
            only_mine: false,
            focus_filter: false,
            scroll_to: None,
            scrolled: (0.0, 0.0),
            // SAFETY: getuid(2) can't fail.
            me: unsafe { libc::getuid() },
        }
    }

    /// Shows `pid` in the list, chosen, as the Performance view's lists
    /// ask: its app's row if it heads one.
    pub fn reveal(&mut self, model: &Model, pid: u32) {
        self.filter.clear();
        self.picked = Some(if model.app(pid).is_some() {
            Pick::App(pid)
        } else {
            Pick::Proc(pid)
        });
        if let Some(app) = model
            .apps
            .iter()
            .find(|a| a.pid != pid && a.members.iter().any(|(p, _)| *p == pid))
        {
            self.expanded.insert(app.pid);
        }
        let lines = self.lines(model, &mut None);
        self.scroll_to = lines
            .iter()
            .position(|l| l.pick(&model.apps) == self.picked);
    }

    pub fn toggle(&mut self, pid: u32) {
        if !self.expanded.remove(&pid) {
            self.expanded.insert(pid);
        }
    }

    pub fn is_expanded(&self, pid: u32) -> bool {
        self.expanded.contains(&pid)
    }

    fn shown(&self, p: &Proc) -> bool {
        (self.show_kernel || !p.kernel) && (!self.only_mine || p.uid == Some(self.me))
    }

    /// The list's lines, in order. `names` gives the apps' names, when the
    /// list is sorted by them or filtered.
    fn lines(&self, model: &Model, names: &mut Option<(&mut Icons, &egui::Context)>) -> Vec<Line> {
        let filter = self.filter.trim().to_lowercase();
        let mut lines = Vec::new();

        let mut apps: Vec<(usize, String)> = model
            .apps
            .iter()
            .enumerate()
            .map(|(i, app)| {
                let name = match names {
                    Some((icons, ctx)) => app_name(app, icons, ctx),
                    None => app.app_id.clone(),
                };
                (i, name)
            })
            .filter(|(i, name)| {
                let app = &model.apps[*i];
                filter.is_empty()
                    || name.to_lowercase().contains(&filter)
                    || app
                        .titles
                        .iter()
                        .any(|t| t.to_lowercase().contains(&filter))
                    || app.members.iter().any(|(pid, _)| {
                        model
                            .process(*pid)
                            .is_some_and(|p| groups::matches(p, &filter))
                    })
            })
            .collect();
        apps.sort_by(|(a, a_name), (b, b_name)| {
            let (a, b) = (&model.apps[*a], &model.apps[*b]);
            match (model.process(a.pid), model.process(b.pid)) {
                (Some(pa), Some(pb)) => self
                    .sort
                    .compare((pa, a_name, a.totals), (pb, b_name, b.totals)),
                _ => a.pid.cmp(&b.pid),
            }
        });
        if !apps.is_empty() {
            lines.push(Line::Heading("Apps", apps.len()));
        }
        for (i, _) in apps {
            let app = &model.apps[i];
            lines.push(Line::App(i));
            if self.expanded.contains(&app.pid) {
                for &(pid, depth) in &app.members {
                    lines.push(Line::Proc {
                        pid,
                        depth: depth + 1,
                    });
                }
            }
        }

        let mut background: Vec<&Proc> = model
            .background()
            .filter(|p| self.shown(p) && groups::matches(p, &filter))
            .collect();
        background.sort_by(|a, b| {
            self.sort
                .compare((a, &a.name, Totals::of(a)), (b, &b.name, Totals::of(b)))
        });
        if !background.is_empty() {
            lines.push(Line::Heading("Background processes", background.len()));
        }
        lines.extend(background.into_iter().map(|p| Line::Proc {
            pid: p.pid,
            depth: 0,
        }));
        lines
    }

    pub fn ui(&mut self, ui: &mut Ui, model: &Model, icons: &mut Icons) -> Vec<Do> {
        let ctx = ui.ctx().clone();
        let mut asked = Vec::new();

        ui.horizontal(|ui| {
            let edit = egui::TextEdit::singleline(&mut self.filter)
                .hint_text("Filter by name, command, user or PID")
                .desired_width(ui.available_width().min(360.0));
            let response = ui.add(edit);
            if std::mem::take(&mut self.focus_filter) {
                response.request_focus();
            }
            if response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.filter.clear();
                response.surrender_focus();
            }
            if !self.filter.is_empty() && ui.small_button("✕").on_hover_text("Clear").clicked() {
                self.filter.clear();
            }
        });
        ui.add_space(6.0);

        let lines = self.lines(model, &mut Some((icons, &ctx)));
        self.keys(ui, model, &lines, &mut asked);
        let with_gpu = !model.snapshot.gpus.is_empty();
        self.header(ui, with_gpu, &mut asked);

        let mut scroll = egui::ScrollArea::vertical().auto_shrink(false);
        if let Some(index) = self.scroll_to.take() {
            let (offset, height) = self.scrolled;
            let (top, bottom) = (index as f32 * ROW, (index + 1) as f32 * ROW);
            if top < offset {
                scroll = scroll.vertical_scroll_offset(top);
            } else if bottom > offset + height {
                scroll = scroll.vertical_scroll_offset(bottom - height);
            }
        }
        let spacing = ui.spacing().item_spacing.y;
        let output = ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            scroll.show_rows(ui, ROW, lines.len(), |ui, range| {
                for i in range {
                    self.row(ui, model, icons, lines[i], &mut asked);
                }
            })
        });
        let output = output.inner;
        self.scrolled = (output.state.offset.y, output.inner_rect.height());
        ui.spacing_mut().item_spacing.y = spacing;
        asked
    }

    /// The arrows move along the list, Right and Left open and close an
    /// app, Enter switches to it, Delete ends and Shift+Delete kills.
    fn keys(&mut self, ui: &Ui, model: &Model, lines: &[Line], asked: &mut Vec<Do>) {
        if ui.ctx().memory(|m| m.focused().is_some()) {
            return;
        }
        let picks: Vec<Pick> = lines.iter().filter_map(|l| l.pick(&model.apps)).collect();
        let at = self.picked.and_then(|p| picks.iter().position(|q| *q == p));
        let (down, up, home, end, right, left, enter, delete, shift) = ui.input(|i| {
            (
                i.key_pressed(egui::Key::ArrowDown),
                i.key_pressed(egui::Key::ArrowUp),
                i.key_pressed(egui::Key::Home),
                i.key_pressed(egui::Key::End),
                i.key_pressed(egui::Key::ArrowRight),
                i.key_pressed(egui::Key::ArrowLeft),
                i.key_pressed(egui::Key::Enter),
                i.key_pressed(egui::Key::Delete),
                i.modifiers.shift,
            )
        });
        let to = if down {
            Some(at.map_or(0, |a| (a + 1).min(picks.len().saturating_sub(1))))
        } else if up {
            Some(at.map_or(0, |a| a.saturating_sub(1)))
        } else if home {
            Some(0)
        } else if end {
            Some(picks.len().saturating_sub(1))
        } else {
            None
        };
        if let Some(pick) = to.and_then(|i| picks.get(i)) {
            asked.push(Do::Pick(*pick));
            self.scroll_to = lines
                .iter()
                .position(|l| l.pick(&model.apps) == Some(*pick));
        }
        let Some(picked) = self.picked else {
            return;
        };
        match picked {
            Pick::App(pid) if right && !self.expanded.contains(&pid) => asked.push(Do::Toggle(pid)),
            Pick::App(pid) if left && self.expanded.contains(&pid) => asked.push(Do::Toggle(pid)),
            _ => {}
        }
        if enter {
            asked.push(Do::Open(picked));
        }
        if delete {
            let signal = if shift { Signal::Kill } else { Signal::End };
            asked.push(Do::Signal(picked, signal));
        }
    }

    fn header(&self, ui: &mut Ui, with_gpu: bool, asked: &mut Vec<Do>) {
        let v = ui.visuals().clone();
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::hover());
        for (column, cell) in cells(rect, with_gpu) {
            let response = ui.interact(
                cell,
                ui.id().with(("column", column.title())),
                Sense::click(),
            );
            let sorted = self.sort.column == column;
            let arrow = match (sorted, self.sort.descending) {
                (false, _) => "",
                (true, true) => " ⏷",
                (true, false) => " ⏶",
            };
            let colour = if sorted || response.hovered() {
                v.strong_text_color()
            } else {
                v.text_color()
            };
            let (align, at) = alignment(column, cell);
            ui.painter().text(
                at,
                align,
                format!("{}{arrow}", column.title()),
                egui::TextStyle::Body.resolve(ui.style()),
                colour,
            );
            if response.clicked() {
                asked.push(Do::Sort(column));
            }
        }
        ui.painter().hline(
            rect.x_range(),
            rect.bottom() - 0.5,
            v.widgets.noninteractive.bg_stroke,
        );
    }

    fn row(&self, ui: &mut Ui, model: &Model, icons: &mut Icons, line: Line, asked: &mut Vec<Do>) {
        let total_memory = model.snapshot.memory.total.max(1);
        let with_gpu = !model.snapshot.gpus.is_empty();
        let v = ui.visuals().clone();
        let body = egui::TextStyle::Body.resolve(ui.style());
        let small = egui::TextStyle::Small.resolve(ui.style());
        let (rect, response) =
            ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
        if let Line::Heading(title, count) = line {
            ui.painter().text(
                pos2(rect.left() + 4.0, rect.bottom() - 4.0),
                Align2::LEFT_BOTTOM,
                format!("{title} ({count})"),
                body,
                v.strong_text_color(),
            );
            return;
        }
        let Some(pick) = line.pick(&model.apps) else {
            return;
        };
        let picked = self.picked == Some(pick);
        if picked {
            ui.painter().rect_filled(rect, 2.0, v.selection.bg_fill);
        } else if response.hovered() {
            ui.painter()
                .rect_filled(rect, 2.0, v.widgets.hovered.weak_bg_fill);
        }
        let text = if picked {
            v.selection.stroke.color
        } else {
            v.text_color()
        };
        let weak = if picked { text } else { v.weak_text_color() };

        // What the row is: an app's own process and the whole app's use,
        // or a process's.
        let (proc, totals, app) = match line {
            Line::App(i) => {
                let app = &model.apps[i];
                (model.process(app.pid), app.totals, Some(app))
            }
            Line::Proc { pid, .. } => {
                let p = model.process(pid);
                (p, p.map(Totals::of).unwrap_or_default(), None)
            }
            Line::Heading(..) => return,
        };
        let Some(proc) = proc else {
            return;
        };

        for (column, cell) in cells(rect, with_gpu) {
            let (align, at) = alignment(column, cell);
            // How much it uses, as a tint behind the number.
            let heat = match column {
                Column::Cpu => (totals.cpu / 100.0).sqrt(),
                Column::Memory => (totals.memory as f32 / total_memory as f32).sqrt(),
                Column::Disk => ((totals.disk / DISK_FULL) as f32).sqrt(),
                Column::Gpu => (totals.gpu / 100.0).sqrt(),
                _ => 0.0,
            };
            if heat > 0.02 && !picked {
                ui.painter().rect_filled(
                    cell.shrink2(vec2(1.0, 1.0)),
                    0.0,
                    v.selection.bg_fill.gamma_multiply(heat.min(1.0) * 0.45),
                );
            }
            match column {
                Column::Name => self.name(ui, cell, line, proc, app, icons, text, weak, asked),
                Column::Pid => {
                    ui.painter()
                        .text(at, align, proc.pid.to_string(), small.clone(), weak);
                }
                Column::User => {
                    ui.painter()
                        .text(at, align, &proc.user, small.clone(), weak);
                }
                Column::Cpu => {
                    let value = if totals.cpu < 0.05 {
                        "0%".to_string()
                    } else if totals.cpu < 10.0 {
                        format!("{:.1}%", totals.cpu)
                    } else {
                        format!("{:.0}%", totals.cpu)
                    };
                    ui.painter().text(at, align, value, body.clone(), text);
                }
                Column::Memory => {
                    let value = crate::graph::bytes(totals.memory as f64);
                    ui.painter().text(at, align, value, body.clone(), text);
                }
                Column::Disk => {
                    let value = if totals.disk < 1.0 {
                        "0".to_string()
                    } else {
                        crate::graph::rate(totals.disk)
                    };
                    ui.painter().text(at, align, value, body.clone(), text);
                }
                Column::Gpu => {
                    let value = if totals.gpu < 0.5 {
                        "0%".to_string()
                    } else {
                        format!("{:.0}%", totals.gpu)
                    };
                    ui.painter().text(at, align, value, body.clone(), text);
                }
            }
        }

        let hover = if totals.gpu_memory > 0 {
            format!(
                "{}\nGPU memory: {}",
                proc.command,
                crate::graph::bytes(totals.gpu_memory as f64)
            )
        } else {
            proc.command.clone()
        };
        let response = response.on_hover_text(hover);
        if response.clicked() || response.secondary_clicked() {
            asked.push(Do::Pick(pick));
        }
        if response.double_clicked() {
            asked.push(Do::Open(pick));
        }
        response.context_menu(|ui| {
            for d in menu(ui, model, pick, self.expanded.contains(&proc.pid)) {
                asked.push(d);
            }
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn name(
        &self,
        ui: &mut Ui,
        cell: Rect,
        line: Line,
        proc: &Proc,
        app: Option<&App>,
        icons: &mut Icons,
        text: Color32,
        weak: Color32,
        asked: &mut Vec<Do>,
    ) {
        let body = egui::TextStyle::Body.resolve(ui.style());
        let small = egui::TextStyle::Small.resolve(ui.style());
        let painter = ui.painter_at(cell);
        let mut x = cell.left() + 4.0;
        let y = cell.center().y;
        let label = match (line, app) {
            (Line::App(_), Some(app)) => {
                // The arrow opening its processes.
                let arrow = Rect::from_center_size(pos2(x + 6.0, y), vec2(14.0, ROW));
                let open = self.expanded.contains(&app.pid);
                let response = ui.interact(arrow, ui.id().with(("open", app.pid)), Sense::click());
                painter.text(
                    arrow.center(),
                    Align2::CENTER_CENTER,
                    if open { "⏷" } else { "⏵" },
                    small.clone(),
                    if response.hovered() { text } else { weak },
                );
                if response.clicked() {
                    asked.push(Do::Toggle(app.pid));
                }
                x += 16.0;
                let ctx = ui.ctx().clone();
                if let Some(icon) = &icons.get(&ctx, &app.app_id).icon {
                    let at = Rect::from_min_size(pos2(x, y - ICON / 2.0), vec2(ICON, ICON));
                    egui::Image::from_texture(icon).paint_at(ui, at);
                }
                x += ICON + 6.0;
                let name = app_name(app, icons, &ctx);
                let title = app.titles.first().filter(|t| !t.is_empty() && **t != name);
                let more = match app.members.len() {
                    1 => String::new(),
                    n => format!(" ({n})"),
                };
                (format!("{name}{more}"), title.cloned())
            }
            (Line::Proc { depth, .. }, _) => {
                x += depth as f32 * INDENT;
                (proc.name.clone(), None)
            }
            _ => return,
        };
        let state = match proc.state {
            State::Stopped => Some("paused"),
            State::Zombie => Some("exited"),
            _ => None,
        };
        let name = painter.text(pos2(x, y), Align2::LEFT_CENTER, label.0, body, text);
        let mut x = name.right() + 8.0;
        if let Some(state) = state {
            let colour = ui.visuals().warn_fg_color;
            let r = painter.text(
                pos2(x, y),
                Align2::LEFT_CENTER,
                state,
                small.clone(),
                colour,
            );
            x = r.right() + 8.0;
        }
        if let Some(title) = label.1 {
            painter.text(pos2(x, y), Align2::LEFT_CENTER, title, small, weak);
        }
    }
}

/// The right click's menu for `pick`, also the Process menu's.
pub fn menu(ui: &mut Ui, model: &Model, pick: Pick, expanded: bool) -> Vec<Do> {
    let mut asked = Vec::new();
    let mut item = |ui: &mut Ui, label: &str, keys: &str, enabled: bool, d: Do| {
        let button = egui::Button::new(label).shortcut_text(keys);
        if ui.add_enabled(enabled, button).clicked() {
            asked.push(d);
            ui.close();
        }
    };
    let pid = match pick {
        Pick::App(p) | Pick::Proc(p) => p,
    };
    let kernel = model.process(pid).is_some_and(|p| p.kernel);
    item(ui, "Details…", "Enter", true, Do::Open(pick));
    if let Pick::App(pid) = pick {
        item(ui, "Switch To", "", true, Do::Switch(pid));
        item(
            ui,
            if expanded {
                "Hide Processes"
            } else {
                "Show Processes"
            },
            if expanded { "←" } else { "→" },
            true,
            Do::Toggle(pid),
        );
        ui.separator();
    }
    let end = match pick {
        Pick::App(_) => "End App",
        Pick::Proc(_) => "End Process",
    };
    item(ui, end, "Delete", !kernel, Do::Signal(pick, Signal::End));
    item(
        ui,
        "Kill",
        "Shift+Delete",
        !kernel,
        Do::Signal(pick, Signal::Kill),
    );
    if model.paused(pick) {
        item(ui, "Resume", "", !kernel, Do::Signal(pick, Signal::Resume));
    } else {
        item(ui, "Pause", "", !kernel, Do::Signal(pick, Signal::Pause));
    }
    ui.separator();
    item(ui, "Copy PID", "", true, Do::Copy(pid.to_string()));
    if let Some(p) = model.process(pid) {
        item(
            ui,
            "Copy Command Line",
            "",
            true,
            Do::Copy(p.command.clone()),
        );
    }
    asked
}

/// The columns' cells across a row's `rect`.
/// Without a GPU, there's no GPU column.
fn cells(rect: Rect, with_gpu: bool) -> impl Iterator<Item = (Column, Rect)> {
    let columns = Column::ALL
        .into_iter()
        .filter(move |c| with_gpu || *c != Column::Gpu);
    let fixed: f32 = columns.clone().map(width).sum();
    let name = (rect.width() - fixed).max(120.0);
    let mut x = rect.left();
    columns.map(move |column| {
        let w = if column == Column::Name {
            name
        } else {
            width(column)
        };
        let cell = Rect::from_x_y_ranges(x..=x + w, rect.y_range());
        x += w;
        (column, cell)
    })
}

/// Where a column's text goes: numbers to the right, words to the left.
fn alignment(column: Column, cell: Rect) -> (Align2, egui::Pos2) {
    match column {
        Column::Name | Column::User => (
            Align2::LEFT_CENTER,
            pos2(cell.left() + 4.0, cell.center().y),
        ),
        _ => (
            Align2::RIGHT_CENTER,
            pos2(cell.right() - 8.0, cell.center().y),
        ),
    }
}

/// The status bar's summary.
pub fn summary(model: &Model) -> String {
    let s = &model.snapshot;
    let memory = s.memory.used as f64 / s.memory.total.max(1) as f64 * 100.0;
    let mut text = format!(
        "{} processes · {} apps · CPU {:.0}% · Memory {memory:.0}%",
        s.processes.len(),
        model.apps.len(),
        s.cpu
    );
    for (i, gpu) in s.gpus.iter().enumerate() {
        let name = if s.gpus.len() == 1 {
            "GPU".to_string()
        } else {
            format!("GPU {i}")
        };
        text.push_str(&format!(" · {name} {:.0}%", gpu.busy.unwrap_or(0.0)));
    }
    text
}

#[cfg(test)]
pub(crate) mod tests {
    use std::time::Instant;

    use patina_system::windows::Window;

    use super::*;
    use crate::sample::Memory;

    pub fn proc(pid: u32, parent: u32, name: &str, cpu: f32) -> Proc {
        Proc {
            pid,
            parent: Some(parent),
            name: name.into(),
            command: name.into(),
            user: "divan".into(),
            uid: Some(1000),
            cpu,
            memory: 1 << 20,
            read: 0.0,
            written: 0.0,
            state: State::Sleeping,
            started: 0,
            kernel: false,
            gpu: 0.0,
            gpu_memory: 0,
        }
    }

    pub fn snapshot() -> Snapshot {
        let mut kworker = proc(5, 2, "kworker/0:1", 0.0);
        kworker.kernel = true;
        Snapshot {
            at: Instant::now(),
            processes: vec![
                proc(10, 1, "patina-compositor", 3.0),
                proc(30, 10, "firefox", 5.0),
                proc(31, 30, "Isolated Web Co", 20.0),
                proc(40, 1, "pipewire", 1.0),
                proc(41, 1, "wireplumber", 0.0),
                kworker,
            ],
            windows: Ok(vec![Window {
                app_id: "firefox".into(),
                title: "Mozilla Firefox".into(),
                pid: 30,
            }]),
            cpu: 30.0,
            cores: vec![30.0],
            cpu_name: String::new(),
            frequency: 0,
            physical_cores: None,
            memory: Memory {
                total: 1 << 30,
                ..Default::default()
            },
            disk_read: 0.0,
            disk_written: 0.0,
            volumes: Vec::new(),
            interfaces: Vec::new(),
            load: [0.0; 3],
            uptime: 0,
            gpus: Vec::new(),
            host: String::new(),
            os: String::new(),
            kernel: String::new(),
            details: Default::default(),
            pressure: None,
        }
    }

    fn picks(view: &ProcessesView, model: &Model) -> Vec<Line> {
        view.lines(model, &mut None)
    }

    #[test]
    fn apps_come_first_then_the_background_by_cpu_without_kernel_threads() {
        let model = Model::new(snapshot());
        let view = ProcessesView::new();
        assert_eq!(
            picks(&view, &model),
            [
                Line::Heading("Apps", 1),
                Line::App(0),
                Line::Heading("Background processes", 3),
                Line::Proc { pid: 10, depth: 0 },
                Line::Proc { pid: 40, depth: 0 },
                Line::Proc { pid: 41, depth: 0 },
            ]
        );
    }

    #[test]
    fn an_open_app_lists_its_processes_under_it() {
        let model = Model::new(snapshot());
        let mut view = ProcessesView::new();
        view.expanded.insert(30);
        let lines = picks(&view, &model);
        assert_eq!(
            lines[1..4],
            [
                Line::App(0),
                Line::Proc { pid: 30, depth: 1 },
                Line::Proc { pid: 31, depth: 2 },
            ]
        );
    }

    #[test]
    fn the_filter_keeps_an_app_with_a_matching_process() {
        let model = Model::new(snapshot());
        let mut view = ProcessesView::new();
        view.filter = "isolated".into();
        assert_eq!(
            picks(&view, &model),
            [Line::Heading("Apps", 1), Line::App(0)]
        );
        view.filter = "wire".into();
        assert_eq!(
            picks(&view, &model),
            [
                Line::Heading("Background processes", 2),
                Line::Proc { pid: 40, depth: 0 },
                Line::Proc { pid: 41, depth: 0 },
            ]
        );
    }

    #[test]
    fn ending_an_app_ends_its_process_and_killing_it_kills_them_all() {
        let model = Model::new(snapshot());
        assert_eq!(model.targets(Pick::App(30), Signal::End), [30]);
        assert_eq!(model.targets(Pick::App(30), Signal::Kill), [30, 31]);
        assert_eq!(model.targets(Pick::Proc(31), Signal::Kill), [31]);
    }

    #[test]
    fn revealing_a_process_in_an_app_opens_the_app() {
        let model = Model::new(snapshot());
        let mut view = ProcessesView::new();
        view.filter = "pipewire".into();
        view.reveal(&model, 31);
        assert!(view.filter.is_empty());
        assert!(view.expanded.contains(&30));
        assert_eq!(view.picked, Some(Pick::Proc(31)));
        assert_eq!(view.scroll_to, Some(3));
    }

    #[test]
    fn without_the_compositor_everything_is_in_the_background() {
        let mut s = snapshot();
        s.windows = Err("no bus".into());
        let model = Model::new(s);
        assert!(model.apps.is_empty());
        assert_eq!(model.background().count(), 6);
    }
}
