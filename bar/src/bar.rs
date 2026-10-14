//! The bar: dwl's status laid out in modules, left, centre and right, as
//! waybar does, in the colours of the active Patina skin.

use std::cell::RefCell;
use std::fmt::Write as _;

use egui::{Align, Color32, Layout, Rect, Sense, Stroke, Ui, UiBuilder, vec2};
use patina::skin::{self, Skin};

use crate::config::{Config, Position};
use crate::dwl::{Monitor, Status};
use patina::tabs::{TabAction, TabStrip};
use patina_shell::{Shell, Toplevel, ToplevelRequest};

/// Space between modules, and at the bar's ends.
const GAP: f32 = 12.0;

pub struct Bar {
    pub config: Config,
    pub status: Status,
    /// The default output's volume, for the tray's speaker.
    pub volume: Option<patina_shell::audio::Volume>,
    /// The Wi-Fi signal, for the tray's Wi-Fi icon (see tray::wifi).
    pub wifi: Option<Option<u8>>,
    /// The battery, for the tray; `None` without one.
    pub battery: Option<patina_shell::power::Battery>,
    skin: Skin,
    /// The open windows, from the compositor, in the order they opened.
    toplevels: Vec<Toplevel>,
    /// Clicks on the window tabs, for the host to pass on.
    requests: RefCell<Vec<ToplevelRequest>>,
}

impl Bar {
    /// Finds `config.skin` among Patina's built-ins and the bar's own skins
    /// folder. Problems are returned, never fatal.
    pub fn new(config: Config) -> (Self, Vec<String>) {
        let dirs: Vec<_> = skin::app_dir("patina-bar").into_iter().collect();
        let (skins, mut warnings) = skin::load_all(skin::BUILT_IN, &dirs);
        let skin = match skins.into_iter().find(|s| s.name == config.skin) {
            Some(s) => s,
            None => {
                if config.skin != skin::DEFAULT_SKIN {
                    warnings.push(format!(
                        "no skin called \"{}\"; using {}",
                        config.skin,
                        skin::DEFAULT_SKIN
                    ));
                }
                Skin::default_skin()
            }
        };
        warnings.extend(
            skin.warnings
                .iter()
                .map(|w| format!("skin \"{}\": {w}", skin.name)),
        );
        (
            Self {
                config,
                status: Status::default(),
                volume: None,
                wifi: None,
                battery: None,
                skin,
                toplevels: Vec::new(),
                requests: RefCell::new(Vec::new()),
            },
            warnings,
        )
    }

    /// The monitor dwl calls `output`, or, if dwl hasn't mentioned it (a
    /// compositor that names outputs differently), the selected one.
    fn monitor(&self, output: &str) -> Monitor {
        self.status
            .monitors
            .get(output)
            .or_else(|| self.status.selected().map(|(_, m)| m))
            .cloned()
            .unwrap_or_default()
    }

    fn module(&self, ui: &mut Ui, name: &str, m: &Monitor) {
        // Tags and the layout are dwl's; with nothing reporting them, as
        // under patina-compositor, they would only be nine dead buttons.
        let from_dwl = matches!(name, "tags" | "layout");
        if from_dwl && self.status.monitors.is_empty() {
            return;
        }
        match name {
            "tags" => self.tags(ui, m),
            "layout" => {
                ui.label(&m.layout);
            }
            "windows" => self.windows(ui),
            "title" => {
                ui.add(egui::Label::new(&m.title).truncate());
            }
            "appid" => {
                ui.label(&m.appid);
            }
            "tray" => {
                let edge = match self.config.position {
                    Position::Top => "top",
                    Position::Bottom => "bottom",
                };
                // The tray's own spacing, tighter than between modules.
                ui.scope(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    crate::tray::wifi(ui, self.wifi, &self.config.wifi, &self.skin.name, edge);
                    crate::tray::speaker(
                        ui,
                        self.volume,
                        &self.config.volume,
                        &self.skin.name,
                        edge,
                    );
                    ui.add_space(4.0);
                    crate::tray::battery(ui, self.battery);
                });
            }
            "launcher" => {
                let edge = match self.config.position {
                    Position::Top => "top",
                    Position::Bottom => "bottom",
                };
                crate::launcher::button(ui, &self.config.launcher, &self.skin.name, edge)
            }
            "clock" => {
                ui.label(clock(&self.config.clock));
            }
            "load" => {
                if let Some(load) = load() {
                    ui.label(format!("load {load:.2}"));
                }
            }
            _ => {}
        }
    }

    /// dwm's tags: the viewed ones in the selection colour, a square on
    /// occupied ones (filled where the focused window is), urgent ones in
    /// the error colour.
    fn tags(&self, ui: &mut Ui, m: &Monitor) {
        let v = ui.visuals().clone();
        let font = egui::TextStyle::Body.resolve(ui.style());
        // In a scope, so the tags sit edge to edge but the next module still
        // gets the usual gap.
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for (i, label) in self.config.tags.iter().enumerate().take(32) {
                let bit = 1u32 << i;
                let (occupied, viewed, client, urgent) = (
                    m.tags.occupied & bit != 0,
                    m.tags.selected & bit != 0,
                    m.tags.client & bit != 0,
                    m.tags.urgent & bit != 0,
                );
                if self.config.hide_vacant_tags && !occupied && !viewed {
                    continue;
                }
                let (fill, text) = if urgent {
                    (v.error_fg_color, v.extreme_bg_color)
                } else if viewed {
                    (v.selection.bg_fill, v.selection.stroke.color)
                } else if occupied {
                    (Color32::TRANSPARENT, v.strong_text_color())
                } else {
                    (Color32::TRANSPARENT, v.weak_text_color())
                };
                let galley = ui
                    .painter()
                    .layout_no_wrap(label.clone(), font.clone(), text);
                let width = (galley.size().x + 14.0).max(ui.available_height());
                let (rect, response) =
                    ui.allocate_exact_size(vec2(width, ui.available_height()), Sense::hover());
                let fill = if response.hovered() && fill == Color32::TRANSPARENT {
                    v.widgets.hovered.weak_bg_fill
                } else {
                    fill
                };
                ui.painter().rect_filled(rect, 0.0, fill);
                ui.painter()
                    .galley(rect.center() - galley.size() / 2.0, galley, text);
                if occupied {
                    let square = Rect::from_min_size(rect.min + vec2(3.0, 3.0), vec2(4.0, 4.0));
                    if client {
                        ui.painter().rect_filled(square, 0.0, text);
                    } else {
                        ui.painter().rect_stroke(
                            square,
                            0.0,
                            Stroke::new(1.0, text),
                            egui::StrokeKind::Inside,
                        );
                    }
                }
            }
        });
    }

    fn row(
        &self,
        ui: &mut Ui,
        rect: Rect,
        layout: Layout,
        modules: &[String],
        m: &Monitor,
        sizing: bool,
    ) -> Rect {
        let mut builder = UiBuilder::new().max_rect(rect).layout(layout);
        if sizing {
            builder = builder.sizing_pass().invisible();
        }
        let mut child = ui.new_child(builder);
        child.spacing_mut().item_spacing.x = GAP;
        for name in modules {
            self.module(&mut child, name, m);
        }
        child.min_rect()
    }
}

impl Bar {
    /// The open windows as Patina tabs, the active one shown: a click
    /// activates a window, its cross or a middle click closes it.
    fn windows(&self, ui: &mut Ui) {
        if self.toplevels.is_empty() {
            return;
        }
        let titles: Vec<&str> = self
            .toplevels
            .iter()
            .map(|t| {
                if t.title.is_empty() {
                    &t.app_id
                } else {
                    &t.title
                }
            })
            .map(String::as_str)
            .collect();
        let shown = self
            .toplevels
            .iter()
            .position(|t| t.activated)
            .unwrap_or(usize::MAX);
        // The bar is the tabs' "title bar", so the shown tab is set off
        // from it the way TabStrip sets tabs off a panel-coloured bar.
        let mut style = self.skin.frame_style(ui.visuals());
        style.title_bar = ui.visuals().panel_fill;
        let span = ui.max_rect().y_range();
        // TabStrip zeroes the item spacing; keep that from the next module.
        let action = ui
            .scope(|ui| {
                TabStrip::new(&titles, shown, &style)
                    .id_salt("windows")
                    .new_tab(false)
                    .noun("Window")
                    .span(span)
                    .show(ui)
            })
            .inner;
        let request = match action {
            Some(TabAction::Show(i)) => self
                .toplevels
                .get(i)
                .map(|t| ToplevelRequest::Activate(t.id)),
            Some(TabAction::Close(i)) => {
                self.toplevels.get(i).map(|t| ToplevelRequest::Close(t.id))
            }
            _ => None,
        };
        if let Some(request) = request {
            self.requests.borrow_mut().push(request);
        }
    }
}

impl Shell for Bar {
    fn toplevels_changed(&mut self, toplevels: &[Toplevel]) {
        self.toplevels = toplevels.to_vec();
    }

    fn take_toplevel_requests(&mut self) -> Vec<ToplevelRequest> {
        std::mem::take(self.requests.get_mut())
    }

    fn setup(&mut self, ctx: &egui::Context) {
        self.skin.apply(ctx);
        if self.skin.follows_desktop() {
            patina::appearance::watch(ctx);
        }
    }

    fn ui(&mut self, ui: &mut Ui, output: &str) {
        let m = self.monitor(output);
        let rect = ui.max_rect();
        let v = ui.visuals();
        ui.painter().rect_filled(rect, 0.0, v.panel_fill);
        let line = v.widgets.noninteractive.bg_stroke;
        let y = match self.config.position {
            Position::Top => rect.bottom() - line.width / 2.0,
            Position::Bottom => rect.top() + line.width / 2.0,
        };
        ui.painter().hline(rect.x_range(), y, line);

        let inner = rect.shrink2(vec2(GAP, 0.0));
        let left = self.row(
            ui,
            inner,
            Layout::left_to_right(Align::Center),
            &self.config.left,
            &m,
            false,
        );
        // right_to_left lays out from the right edge, so reverse the modules
        // to keep them in the order the config lists them.
        let right_modules: Vec<String> = self.config.right.iter().rev().cloned().collect();
        let right = self.row(
            ui,
            inner,
            Layout::right_to_left(Align::Center),
            &right_modules,
            &m,
            false,
        );

        if self.config.center.is_empty() {
            return;
        }
        let left_end = if self.config.left.is_empty() {
            inner.left()
        } else {
            left.right() + GAP
        };
        let right_start = if self.config.right.is_empty() {
            inner.right()
        } else {
            right.left() - GAP
        };
        if right_start <= left_end {
            return;
        }
        // Measure the centre modules, then centre them on the bar where
        // there's room, or as near it as the side modules allow.
        let free = Rect::from_x_y_ranges(left_end..=right_start, inner.y_range());
        let width = self
            .row(
                ui,
                free,
                Layout::left_to_right(Align::Center),
                &self.config.center,
                &m,
                true,
            )
            .width()
            .min(free.width());
        let x = (rect.center().x - width / 2.0).clamp(free.left(), free.right() - width);
        let centre = Rect::from_x_y_ranges(x..=free.right(), inner.y_range());
        self.row(
            ui,
            centre,
            Layout::left_to_right(Align::Center),
            &self.config.center,
            &m,
            false,
        );
    }
}

/// Local time in `format`, or a note that the format is broken (chrono
/// reports a bad specifier as a formatting error rather than at parse time).
fn clock(format: &str) -> String {
    let mut out = String::new();
    match write!(out, "{}", chrono::Local::now().format(format)) {
        Ok(()) => out,
        Err(_) => format!("bad clock format: {format}"),
    }
}

/// The one-minute load average.
fn load() -> Option<f64> {
    let mut avg = [0.0f64; 3];
    // SAFETY: getloadavg writes at most `nelem` doubles into `avg`.
    let n = unsafe { libc::getloadavg(avg.as_mut_ptr(), 3) };
    (n >= 1).then_some(avg[0])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config;

    fn bar(config_text: &str) -> Bar {
        let (config, _) = config::parse(config_text);
        Bar {
            config,
            status: Status::default(),
            volume: None,
            wifi: None,
            battery: None,
            skin: Skin::default_skin(),
            toplevels: Vec::new(),
            requests: RefCell::new(Vec::new()),
        }
    }

    /// Runs the bar for a 1000x26 output and returns the text it drew, with
    /// each galley's left edge.
    fn texts(bar: &mut Bar, output: &str) -> Vec<(String, f32)> {
        let ctx = egui::Context::default();
        bar.setup(&ctx);
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1000.0, 26.0))),
            ..Default::default()
        };
        let out = ctx.run_ui(input, |ui| bar.ui(ui, output));
        let mut found = Vec::new();
        for clipped in &out.shapes {
            collect(&clipped.shape, &mut found);
        }
        found
    }

    fn collect(shape: &egui::Shape, found: &mut Vec<(String, f32)>) {
        match shape {
            egui::Shape::Text(t) => found.push((t.galley.text().to_string(), t.pos.x)),
            egui::Shape::Vec(v) => v.iter().for_each(|s| collect(s, found)),
            _ => {}
        }
    }

    fn feed(bar: &mut Bar, lines: &[&str]) {
        for l in lines {
            bar.status.apply(l);
        }
    }

    #[test]
    fn shows_the_status_of_its_own_output() {
        let mut b = bar("right = []");
        feed(
            &mut b,
            &[
                "A title on A",
                "A layout []=",
                "A tags 1 1 1 0",
                "B title on B",
                "B selmon 1",
            ],
        );
        let t = texts(&mut b, "A");
        let words: Vec<_> = t.iter().map(|(s, _)| s.as_str()).collect();
        assert!(words.contains(&"on A"), "{words:?}");
        assert!(words.contains(&"[]="));
        assert!(!words.contains(&"on B"));
        assert_eq!(
            words
                .iter()
                .filter(|w| w.len() == 1 && w.chars().all(|c| c.is_ascii_digit()))
                .count(),
            9
        );
    }

    #[test]
    fn unknown_outputs_follow_the_selected_monitor() {
        let mut b = bar("");
        feed(&mut b, &["X title hello", "X selmon 1"]);
        assert!(texts(&mut b, "Y").iter().any(|(s, _)| s == "hello"));
    }

    fn window(id: u64, title: &str, activated: bool) -> Toplevel {
        Toplevel {
            id,
            title: title.into(),
            app_id: format!("app{id}"),
            activated,
        }
    }

    /// Runs frames of the bar at `time` steps, so clicks don't merge.
    fn frame(bar: &mut Bar, ctx: &egui::Context, time: &mut f64, events: Vec<egui::Event>) {
        *time += 0.05;
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1000.0, 41.0))),
            time: Some(*time),
            events,
            ..Default::default()
        };
        let _ = ctx.run_ui(input, |ui| bar.ui(ui, "A"));
    }

    #[test]
    fn open_windows_are_tabs_and_a_click_activates_one() {
        let mut b = bar("center = []\nright = []");
        b.toplevels_changed(&[window(7, "notes", false), window(9, "", true)]);
        let words: Vec<String> = texts(&mut b, "A").into_iter().map(|(s, _)| s).collect();
        assert!(words.contains(&"notes".to_string()), "{words:?}");
        assert!(
            words.contains(&"app9".to_string()),
            "an untitled window shows its app id"
        );

        let ctx = egui::Context::default();
        b.setup(&ctx);
        let mut time = 0.0;
        frame(&mut b, &ctx, &mut time, vec![]);
        let tab = ctx
            .read_response(egui::Id::new("windows").with(("tab", 0usize)))
            .expect("the first window has a tab")
            .rect;
        // Away from its close cross, which shows at the right end on hover.
        let at = tab.left_center() + vec2(12.0, 0.0);
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        frame(&mut b, &ctx, &mut time, vec![egui::Event::PointerMoved(at)]);
        frame(&mut b, &ctx, &mut time, vec![button(true)]);
        frame(&mut b, &ctx, &mut time, vec![button(false)]);
        assert_eq!(b.take_toplevel_requests(), [ToplevelRequest::Activate(7)]);
        assert!(b.take_toplevel_requests().is_empty(), "taken once");
    }

    #[test]
    fn no_windows_no_tabs() {
        let mut b = bar("left = [\"windows\"]\ncenter = []\nright = []");
        assert!(texts(&mut b, "A").is_empty());
    }

    #[test]
    fn no_tags_without_dwl() {
        let mut b = bar("right = []");
        let t = texts(&mut b, "A");
        assert!(t.iter().all(|(s, _)| s.is_empty()), "{t:?}");
    }

    #[test]
    fn hides_vacant_tags() {
        let mut b = bar("hide_vacant_tags = true\ncenter = []\nright = []\nleft = [\"tags\"]");
        feed(&mut b, &["A tags 5 2 0 0"]);
        let t: Vec<_> = texts(&mut b, "A").into_iter().map(|(s, _)| s).collect();
        assert_eq!(t, ["1", "2", "3"]);
    }

    #[test]
    fn centres_the_title_and_keeps_the_right_in_order() {
        let mut b = bar("left = []\nright = [\"layout\", \"appid\"]");
        feed(&mut b, &["A title mid", "A layout LAY", "A appid APP"]);
        let t = texts(&mut b, "A");
        let x = |w: &str| t.iter().find(|(s, _)| s == w).map(|(_, x)| *x).unwrap();
        assert!((x("mid") - 500.0).abs() < 20.0, "title at {}", x("mid"));
        assert!(x("LAY") < x("APP"));
        assert!(x("APP") > 900.0);
    }

    #[test]
    fn a_bad_clock_format_says_so() {
        assert!(clock("%Q").starts_with("bad clock format"));
        assert_eq!(clock("x"), "x");
    }
}
