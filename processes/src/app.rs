//! The window: Patina's frame, with File, View and Process on the left of
//! the title bar and the views as tabs after them (Patina's tabs, fixed:
//! none opened or closed), the view, and the status bar. It wears the desktop's skin, from the bar's
//! config, as Settings does, and follows it.

use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime};

use egui::{RichText, Ui};
use patina::frame::{self, TitleBar};
use patina::skin::{self, Skin};
use patina::tabs::{self, TabAction, TabStrip};

use crate::icons::Icons;
use crate::log_view::LogView;
use crate::performance::{self, History, PerformanceView};
use crate::problems::Go;
use crate::processes::{self, Do, Model, Pick, ProcessesView};
use crate::sample::{self, Ask, Signal, Snapshot};
use crate::window::ProcessWindow;

/// The shell's skins folder, the bar's.
const SHELL: &str = "patina-hud";
const SKIN_POLL: Duration = Duration::from_secs(1);
/// The processes' own windows' app id, apart from the main window's, so
/// Ctrl+Shift+Escape brings that one forward.
const WINDOW_ID: &str = "com.sectorflabs.ProcessManager.Process";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Overview,
    Processes,
    Performance,
    Log,
}

impl View {
    /// In the tabs' order.
    const ALL: [View; 4] = [
        View::Overview,
        View::Processes,
        View::Performance,
        View::Log,
    ];

    fn title(self) -> &'static str {
        match self {
            View::Overview => "Overview",
            View::Processes => "Processes",
            View::Performance => "Performance",
            View::Log => "Log",
        }
    }

    fn index(self) -> usize {
        View::ALL.iter().position(|v| *v == self).unwrap_or(0)
    }
}

/// How often the machine is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Speed {
    Fast,
    Normal,
    Slow,
    Paused,
}

impl Speed {
    const ALL: [Speed; 4] = [Speed::Fast, Speed::Normal, Speed::Slow, Speed::Paused];

    fn every(self) -> Option<Duration> {
        match self {
            Speed::Fast => Some(Duration::from_millis(500)),
            Speed::Normal => Some(Duration::from_secs(1)),
            Speed::Slow => Some(Duration::from_secs(4)),
            Speed::Paused => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Speed::Fast => "Fast (twice a second)",
            Speed::Normal => "Normal (every second)",
            Speed::Slow => "Slow (every 4 seconds)",
            Speed::Paused => "Paused",
        }
    }
}

/// The status bar's latest message.
struct Message {
    text: String,
    error: bool,
}

/// Ending something the desktop runs on, waiting to be confirmed.
struct Confirm {
    pick: Pick,
    signal: Signal,
    name: String,
}

pub struct App {
    custom_frame: bool,
    skins: Vec<Skin>,
    skin: usize,
    bar_mtime: Option<SystemTime>,
    pub view: View,
    speed: Speed,
    ask: mpsc::Sender<Ask>,
    snapshots: mpsc::Receiver<Snapshot>,
    model: Option<Model>,
    history: History,
    started: Instant,
    processes: ProcessesView,
    pub performance: PerformanceView,
    busiest: crate::overview::Busiest,
    /// The processes' own windows open, in the order they were opened.
    windows: Vec<ProcessWindow>,
    /// The kernel's log, followed from the start.
    log: LogView,
    icons: Icons,
    message: Option<Message>,
    /// Errors from switching to a window, which happens on a thread.
    switched: (mpsc::Sender<String>, mpsc::Receiver<String>),
    confirm: Option<Confirm>,
}

impl App {
    pub fn new(ctx: &egui::Context, view: View) -> Self {
        patina::appearance::watch(ctx);
        let dirs: Vec<_> = skin::app_dir(SHELL).into_iter().collect();
        let (skins, _) = skin::load_all(skin::BUILT_IN, &dirs);
        let speed = Speed::Normal;
        let (ask, snapshots) = sample::start(ctx.clone(), speed.every());
        let mut app = Self {
            custom_frame: true,
            skins,
            skin: 0,
            bar_mtime: None,
            view,
            speed,
            ask,
            snapshots,
            model: None,
            history: History::default(),
            started: Instant::now(),
            processes: ProcessesView::new(),
            performance: PerformanceView::new(),
            busiest: crate::overview::Busiest::new(),
            windows: Vec::new(),
            log: LogView::new(ctx),
            icons: Icons::new(ctx),
            message: None,
            switched: mpsc::channel(),
            confirm: None,
        };
        app.follow_shell_skin(ctx);
        app
    }

    /// Takes the shell's skin from the bar's config when it changes.
    fn follow_shell_skin(&mut self, ctx: &egui::Context) {
        let mtime = patina_shell::hud::config_path()
            .and_then(|p| std::fs::metadata(p).ok())
            .and_then(|m| m.modified().ok());
        if mtime == self.bar_mtime && self.bar_mtime.is_some() {
            return;
        }
        self.bar_mtime = mtime;
        let name = patina_shell::hud::skin_or_default();
        self.skin = self.skins.iter().position(|s| s.name == name).unwrap_or(0);
        self.skins[self.skin].apply(ctx);
    }

    fn note(&mut self, text: impl Into<String>) {
        self.message = Some(Message {
            text: text.into(),
            error: false,
        });
    }

    fn error(&mut self, text: impl Into<String>) {
        self.message = Some(Message {
            text: text.into(),
            error: true,
        });
    }

    fn set_speed(&mut self, speed: Speed) {
        self.speed = speed;
        let _ = self.ask.send(Ask::Every(speed.every()));
    }

    fn take_snapshots(&mut self) {
        self.log.take();
        let mut latest = None;
        while let Ok(s) = self.snapshots.try_recv() {
            let model = Model::new(s);
            let at = model.snapshot.at.duration_since(self.started).as_secs_f64();
            self.history.push(at, &model);
            self.busiest.update(&model);
            for w in &mut self.windows {
                w.push(at, &model);
            }
            latest = Some(model);
        }
        if let Some(model) = latest {
            // What was chosen and has gone: nothing chosen.
            let gone = match self.processes.picked {
                Some(Pick::App(pid)) => model.app(pid).is_none(),
                Some(Pick::Proc(pid)) => model.process(pid).is_none(),
                None => false,
            };
            if gone {
                self.processes.picked = None;
            }
            self.model = Some(model);
        }
        while let Ok(e) = self.switched.1.try_recv() {
            self.error(e);
        }
    }

    /// The graphs' "now": the latest reading's time, so they hold still
    /// while paused.
    fn now(&self) -> f64 {
        self.model.as_ref().map_or(0.0, |m| {
            m.snapshot.at.duration_since(self.started).as_secs_f64()
        })
    }

    fn act(&mut self, ctx: &egui::Context, asked: Vec<Do>) {
        for d in asked {
            match d {
                Do::Pick(pick) => self.processes.picked = Some(pick),
                Do::Toggle(pid) => self.processes.toggle(pid),
                Do::Sort(column) => self.processes.sort = self.processes.sort.by(column),
                Do::Copy(text) => {
                    ctx.copy_text(text.clone());
                    self.note(format!("Copied {text}"));
                }
                Do::Switch(pid) => self.switch_to(pid),
                Do::Man(page) => self.open_man(page),
                Do::Open(pick) => self.open_window(ctx, pick),
                Do::Signal(pick, signal) => self.signal(ctx, pick, signal, false),
            }
        }
    }

    /// Opens `pick`'s own window, or brings it forward if it's open.
    fn open_window(&mut self, ctx: &egui::Context, pick: Pick) {
        if let Some(w) = self.windows.iter().find(|w| w.pick == pick) {
            ctx.send_viewport_cmd_to(w.id(), egui::ViewportCommand::Focus);
            return;
        }
        let Some(model) = &self.model else {
            return;
        };
        self.windows
            .push(ProcessWindow::new(pick, model, &mut self.icons, ctx));
        self.watch();
    }

    /// Tells the reader whose details to read: the open windows'.
    fn watch(&self) {
        let mut pids: Vec<u32> = self.windows.iter().map(ProcessWindow::pid).collect();
        pids.dedup();
        let _ = self.ask.send(Ask::Watch(pids));
    }

    /// Each open process window, as a viewport of its own.
    fn process_windows(&mut self, ctx: &egui::Context) {
        let mut asked = Vec::new();
        let mut closed = Vec::new();
        for i in 0..self.windows.len() {
            let w = &self.windows[i];
            let builder = frame::viewport(
                egui::ViewportBuilder::default()
                    .with_title(w.title())
                    .with_app_id(WINDOW_ID)
                    .with_inner_size(crate::window::SIZE)
                    .with_min_inner_size([480.0, 400.0]),
                self.custom_frame,
            );
            ctx.show_viewport_immediate(w.id(), builder, |ui, _| {
                if ui.ctx().input(|i| i.viewport().close_requested()) {
                    closed.push(i);
                }
                asked.extend(self.window_ui(ui, i));
            });
        }
        if !closed.is_empty() {
            let mut i = 0;
            self.windows.retain(|_| {
                i += 1;
                !closed.contains(&(i - 1))
            });
            self.watch();
        }
        self.act(ctx, asked);
    }

    /// The `i`th process window's contents.
    fn window_ui(&mut self, ui: &mut Ui, i: usize) -> Vec<Do> {
        let now = self.now();
        let style = self.skins[self.skin].frame_style(ui.visuals());
        let Some(model) = &self.model else {
            return Vec::new();
        };
        self.windows[i].ui(ui, model, &mut self.icons, &style, self.custom_frame, now)
    }

    /// Opens man page `page` ("journalctl(1)") in Reader, alone, without
    /// its tree of every page (`--single`): started by the compositor, as
    /// the bar's menus start what they open, so it's one of the desktop's
    /// processes; else directly.
    fn open_man(&mut self, page: &'static str) {
        let command = format!("reader --single --man '{page}'");
        if patina_system::processes::spawn(&command, &[]).is_ok() {
            return;
        }
        if let Err(e) = std::process::Command::new("reader")
            .args(["--single", "--man", page])
            .spawn()
        {
            self.error(format!("Couldn't open {page} in Reader: {e}"));
        }
    }

    /// Brings forward `pid`'s window, through the compositor.
    fn switch_to(&mut self, pid: u32) {
        let errors = self.switched.0.clone();
        std::thread::spawn(move || {
            let done = patina_system::windows::Windows::connect().and_then(|w| w.activate(pid));
            if let Err(e) = done {
                let _ = errors.send(format!("Couldn't switch to it: {e}"));
            }
        });
    }

    /// Sends `signal` for `pick`; something the desktop runs on is asked
    /// about first, unless `confirmed`.
    fn signal(&mut self, ctx: &egui::Context, pick: Pick, signal: Signal, confirmed: bool) {
        let Some(model) = &self.model else {
            return;
        };
        let name = model.describe(pick, &mut self.icons, ctx);
        let targets = model.targets(pick, signal);
        let me = std::process::id();
        let under_us = targets.iter().any(|p| *p != me && model.ours.contains(p));
        if under_us && !confirmed && signal != Signal::Resume {
            self.confirm = Some(Confirm { pick, signal, name });
            return;
        }
        let failed: Vec<String> = targets
            .iter()
            .filter_map(|pid| sample::send(*pid, signal).err().map(|e| (pid, e)))
            .map(|(pid, e)| match e.kind() {
                std::io::ErrorKind::PermissionDenied => {
                    let owner = model.process(*pid).map_or("", |p| p.user.as_str());
                    format!("it's {owner}'s, not yours")
                }
                _ => e.to_string(),
            })
            .collect();
        match failed.first() {
            Some(why) => self.error(format!("Couldn't {} {name}: {why}", signal.verb())),
            None => self.note(match signal {
                Signal::End => format!("Asked {name} to end"),
                Signal::Kill => format!("Killed {name}"),
                Signal::Pause => format!("Paused {name}"),
                Signal::Resume => format!("Resumed {name}"),
            }),
        }
        let _ = self.ask.send(Ask::Now);
    }

    fn menus(&mut self, ui: &mut Ui, asked: &mut Vec<Do>) {
        ui.menu_button("File", |ui| {
            if ui
                .add(egui::Button::new("Quit").shortcut_text("Ctrl+Q"))
                .clicked()
            {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
        ui.menu_button("View", |ui| {
            for (i, view) in View::ALL.into_iter().enumerate() {
                let keys = format!("Alt+{}", i + 1);
                let button =
                    egui::Button::selectable(self.view == view, view.title()).shortcut_text(keys);
                if ui.add(button).clicked() {
                    self.view = view;
                    ui.close();
                }
            }
            if ui
                .add(egui::Button::new("Filter…").shortcut_text("Ctrl+F"))
                .clicked()
            {
                self.view = View::Processes;
                self.processes.focus_filter = true;
                ui.close();
            }
            ui.separator();
            ui.menu_button("Update Speed", |ui| {
                for speed in Speed::ALL {
                    if ui.radio(self.speed == speed, speed.label()).clicked() {
                        self.set_speed(speed);
                        ui.close();
                    }
                }
            });
            ui.checkbox(&mut self.processes.only_mine, "Only My Processes");
            ui.checkbox(&mut self.processes.show_kernel, "Kernel Threads");
            ui.separator();
            let mut system = !self.custom_frame;
            if ui.checkbox(&mut system, "System Title Bar").changed() {
                self.custom_frame = !system;
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Decorations(system));
            }
        });
        ui.menu_button("Process", |ui| match (&self.model, self.processes.picked) {
            (Some(model), Some(pick)) => {
                let expanded = matches!(pick, Pick::App(pid) if self.processes.is_expanded(pid));
                asked.extend(processes::menu(ui, model, pick, expanded));
            }
            _ => {
                ui.label(RichText::new("Choose a process in the list").weak());
            }
        });
    }

    /// Ctrl+Q and Ctrl+F; the tabs' own keys (Alt+1 to 3,
    /// Ctrl+PageUp and PageDown) are `tabs::keys`'.
    fn keys(&mut self, ctx: &egui::Context) {
        let (quit, find) = ctx.input(|i| {
            let c = i.modifiers.command;
            (
                c && i.key_pressed(egui::Key::Q),
                c && i.key_pressed(egui::Key::F),
            )
        });
        if quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if find {
            self.view = View::Processes;
            self.processes.focus_filter = true;
        }
    }

    fn status_bar(&self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let weak = ui.visuals().weak_text_color();
            if let Some(model) = &self.model {
                ui.label(RichText::new(processes::summary(model)).small().color(weak));
            }
            if self.speed == Speed::Paused {
                ui.separator();
                ui.label(
                    RichText::new("Paused")
                        .small()
                        .color(ui.visuals().warn_fg_color),
                );
            }
            if let Some(Err(e)) = self.model.as_ref().map(|m| &m.snapshot.windows) {
                ui.separator();
                ui.label(
                    RichText::new("No windows from the compositor")
                        .small()
                        .color(weak),
                )
                .on_hover_text(e);
            }
            if let Some(m) = &self.message {
                ui.separator();
                let colour = if m.error {
                    ui.visuals().error_fg_color
                } else {
                    weak
                };
                ui.label(RichText::new(&m.text).small().color(colour));
            }
        });
    }

    /// Asks before ending something the desktop runs on.
    fn confirm(&mut self, ctx: &egui::Context) {
        let Some(c) = &self.confirm else {
            return;
        };
        let mut answer = None;
        let modal = egui::Modal::new(egui::Id::new("confirm")).show(ctx, |ui| {
            ui.set_max_width(360.0);
            ui.heading(format!("{} {}?", capitalise(c.signal.verb()), c.name));
            ui.add_space(6.0);
            ui.label(
                "The desktop runs on it: this process manager and other windows may close with it.",
            );
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.button(capitalise(c.signal.verb())).clicked() {
                    answer = Some(true);
                }
                if ui.button("Cancel").clicked() {
                    answer = Some(false);
                }
            });
        });
        if modal.should_close() {
            answer.get_or_insert(false);
        }
        if let Some(yes) = answer {
            let c = self.confirm.take().expect("asked");
            if yes {
                self.signal(ctx, c.pick, c.signal, true);
            }
        }
    }

    pub fn draw(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        self.follow_shell_skin(&ctx);
        ctx.request_repaint_after(SKIN_POLL);
        self.take_snapshots();
        self.keys(&ctx);
        let titles = View::ALL.map(View::title);
        let mut tab = tabs::keys(&ctx, titles.len(), self.view.index());
        let style = self.skins[self.skin].frame_style(ui.visuals());
        let radius = if self.custom_frame {
            frame::corner_radius(&ctx, style.radius)
        } else {
            0
        };
        let mut asked = Vec::new();

        egui::Panel::top("header")
            .frame(frame::header_frame(ui.style(), &style, radius))
            .show(ui, |ui| {
                // The bar's full height, for the tabs to fill.
                let span = frame::header_span(ui);
                let shown = self.view.index();
                TitleBar::new("", &style)
                    .custom(self.custom_frame)
                    .menu_bar(true)
                    .span(span)
                    .show(
                        ui,
                        |ui| {
                            self.menus(ui, &mut asked);
                            let strip = TabStrip::new(&titles, shown, &style)
                                .span(span)
                                .new_tab(false)
                                .closable(false);
                            tab = strip.show(ui).or(tab);
                        },
                        |_ui| {},
                    );
            });
        // New and Close don't apply: the views are fixed.
        if let Some(TabAction::Show(i)) = tab
            && let Some(view) = View::ALL.get(i)
        {
            self.view = *view;
        }
        egui::Panel::bottom("status")
            .frame(
                egui::Frame::side_top_panel(ui.style())
                    .inner_margin(egui::Margin::symmetric(10, 4))
                    .corner_radius(egui::CornerRadius {
                        sw: radius,
                        se: radius,
                        ..Default::default()
                    }),
            )
            .show(ui, |ui| self.status_bar(ui));

        let now = self.now();
        match (self.view, &self.model) {
            (_, None) => {
                egui::CentralPanel::default().show(ui, |ui| {
                    ui.centered_and_justified(|ui| ui.label(RichText::new("Reading…").weak()));
                });
            }
            (View::Overview, Some(model)) => {
                let problems = crate::problems::find(model, &self.log.entries);
                let mut asked_here = Vec::new();
                egui::CentralPanel::default()
                    .frame(egui::Frame::central_panel(ui.style()).inner_margin(20))
                    .show(ui, |ui| {
                        asked_here = crate::overview::ui(
                            ui,
                            model,
                            &self.history,
                            &self.busiest,
                            &problems,
                            &mut self.icons,
                            now,
                        );
                    });
                for d in asked_here {
                    match d {
                        crate::overview::Do::Show(resource) => {
                            self.performance.resource = resource;
                            self.view = View::Performance;
                        }
                        // Show Processes for an app: its processes, open.
                        crate::overview::Do::Process(Do::Toggle(pid)) => {
                            self.processes.reveal(model, pid);
                            if !self.processes.is_expanded(pid) {
                                self.processes.toggle(pid);
                            }
                            self.view = View::Processes;
                        }
                        crate::overview::Do::Process(d) => asked.push(d),
                        crate::overview::Do::Go(Go::Log(event, words)) => {
                            self.log.show(event, &words);
                            self.view = View::Log;
                        }
                        crate::overview::Do::Go(Go::Pressure) => {
                            self.performance.resource = performance::Resource::Pressure;
                            self.view = View::Performance;
                        }
                        crate::overview::Do::Go(Go::Nowhere) => {}
                    }
                }
            }
            (View::Processes, Some(model)) => {
                egui::CentralPanel::default()
                    .frame(egui::Frame::central_panel(ui.style()).inner_margin(12))
                    .show(ui, |ui| {
                        asked.extend(self.processes.ui(ui, model, &mut self.icons));
                    });
            }
            (View::Performance, Some(model)) => {
                egui::Panel::left("resources")
                    .frame(egui::Frame::side_top_panel(ui.style()))
                    .default_size(240.0)
                    .resizable(false)
                    .show(ui, |ui| {
                        self.performance.sidebar(ui, model, &self.history, now);
                    });
                let mut reveal = Vec::new();
                egui::CentralPanel::default()
                    .frame(egui::Frame::central_panel(ui.style()).inner_margin(16))
                    .show(ui, |ui| {
                        reveal =
                            self.performance
                                .ui(ui, model, &self.history, &mut self.icons, now);
                    });
                for performance::Do::Open(pick) in reveal {
                    asked.push(Do::Open(pick));
                }
            }
            (View::Log, Some(model)) => {
                egui::CentralPanel::default()
                    .frame(egui::Frame::central_panel(ui.style()).inner_margin(12))
                    .show(ui, |ui| {
                        asked.extend(self.log.ui(ui, model));
                    });
            }
        }
        self.act(&ctx, asked);
        self.process_windows(&ctx);
        self.confirm(&ctx);

        if self.custom_frame {
            frame::edges(&ctx, radius, style.border);
        }
    }
}

fn capitalise(word: &str) -> String {
    let mut c = word.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.draw(ui);
    }

    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        frame::clear_color(visuals, self.custom_frame)
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        frame::release_grab(ctx, raw_input);
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use egui::{Rect, vec2};

    use super::*;

    /// Draws each view in software, as PNGs in the folder
    /// PATINA_PROCESSES_PREVIEW names, from this machine's processes: `cargo
    /// test -p patina-processes preview -- --ignored`.
    #[test]
    #[ignore = "writes files"]
    fn preview_the_views() {
        use crate::performance::Resource;
        let dir = PathBuf::from(std::env::var("PATINA_PROCESSES_PREVIEW").unwrap());
        // Performance at the CPU, the first GPU and the pressure.
        for (view, resource, name) in [
            (View::Overview, None, "overview.png"),
            (View::Processes, None, "processes.png"),
            (View::Performance, None, "performance.png"),
            (View::Performance, Some(Resource::Gpu(0)), "gpu.png"),
            (View::Performance, Some(Resource::Pressure), "pressure.png"),
            (View::Log, None, "log.png"),
            (View::Log, None, "log-crashes.png"),
        ] {
            let ctx = egui::Context::default();
            let mut app = App::new(&ctx, view);
            if let Some(r) = resource {
                app.performance.resource = r;
            }
            if name == "log-crashes.png" {
                app.log.show(Some("Crash"), "");
            }
            app.set_speed(Speed::Fast);
            // Long enough for a few readings, and the icons.
            render(&ctx, [1240, 840], 40, &dir.join(name), |ui| app.draw(ui));
        }
    }

    /// Draws this test's own process window as process.png in
    /// PATINA_PROCESSES_PREVIEW, after a few readings: `cargo test -p
    /// patina-processes preview -- --ignored`.
    #[test]
    #[ignore = "writes a file"]
    fn preview_a_process_window() {
        let dir = PathBuf::from(std::env::var("PATINA_PROCESSES_PREVIEW").unwrap());
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx, View::Overview);
        app.set_speed(Speed::Fast);
        // A first reading, to open it from.
        while app.model.is_none() {
            app.take_snapshots();
            std::thread::sleep(Duration::from_millis(50));
        }
        app.open_window(&ctx, Pick::Proc(std::process::id()));
        let size = crate::window::SIZE.map(|s| s as usize);
        render(&ctx, size, 40, &dir.join("process.png"), |ui| {
            app.take_snapshots();
            app.window_ui(ui, 0);
        });
    }

    /// Runs `draw` `frames` times, a tenth of a second apart, in a window
    /// `size` big, and saves the last as `file`.
    fn render(
        ctx: &egui::Context,
        size: [usize; 2],
        frames: usize,
        file: &std::path::Path,
        mut draw: impl FnMut(&mut Ui),
    ) {
        let [w, h] = size;
        let mut painter = patina_shell::raster::Painter::default();
        let mut pixels = vec![0u32; w * h];
        for _ in 0..frames {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::pos2(0.0, 0.0),
                    vec2(w as f32, h as f32),
                )),
                max_texture_side: Some(8192),
                ..Default::default()
            };
            let out = ctx.run_ui(input, &mut draw);
            let primitives = ctx.tessellate(out.shapes, out.pixels_per_point);
            painter.paint(
                &mut pixels,
                w,
                h,
                out.pixels_per_point,
                &primitives,
                &out.textures_delta,
            );
            std::thread::sleep(Duration::from_millis(100));
        }
        let rgba: Vec<u8> = pixels
            .iter()
            .flat_map(|p| {
                let [b, g, r, a] = p.to_le_bytes();
                [r, g, b, a]
            })
            .collect();
        image::save_buffer(file, &rgba, w as u32, h as u32, image::ColorType::Rgba8).unwrap();
    }
}
