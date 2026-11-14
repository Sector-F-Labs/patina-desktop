//! The column (ADR-0141): one layer-shell surface down the right edge, as wide
//! as a card plus its margin, with the cards from the top. While any are
//! showing it keeps that width as an exclusive zone, so the compositor's work
//! area, and with it tiled windows, make room; with none it is one pixel wide
//! and keeps nothing.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use egui::{
    Align, CornerRadius, Frame, Layout, Margin, RichText, ScrollArea, Sense, Stroke, Ui, UiBuilder,
    vec2,
};
use patina::skin::Skin;
use patina_shell::calloop::channel;
use patina_shell::{
    Anchor, Host, KeyboardInteractivity, Layer, Options, Shell, Toplevel, ToplevelRequest, piece,
};

use crate::daemon::{Daemon, Shared};
use crate::store::{self, Note, Urgency};
use crate::{config, fullscreen};

/// A card's width, and the space around the cards, in logical pixels.
const CARD: f32 = 340.0;
const PAD: f32 = 12.0;
const GAP: f32 = 8.0;
/// The surface's width while there are cards.
const WIDTH: u32 = (CARD + 2.0 * PAD) as u32;
/// How often the config is looked at, and so the longest the column sleeps.
const HEARTBEAT: Duration = Duration::from_secs(1);

/// What was done to a card.
#[derive(Debug, PartialEq)]
enum Outcome {
    Nothing,
    Dismiss,
    /// One of its buttons.
    Action(String),
    /// The card itself: its default action, if it has one, and its app's window.
    Open,
}

struct Column {
    skin: Skin,
    shared: Arc<Shared>,
    daemon: Daemon,
    /// The open windows (id, app id), to bring an app's forward from its card.
    windows: Vec<(u64, String)>,
    requests: Vec<ToplevelRequest>,
    enabled: bool,
    /// Held back while a fullscreen window has the keyboard.
    hide_in_fullscreen: bool,
    fullscreen: Arc<AtomicBool>,
    config_seen: Option<std::time::SystemTime>,
    config_checked: Option<Instant>,
    /// The surface's height and the cards' (with the margin), from the last
    /// frame: what is below the cards takes no pointer input.
    surface_h: f32,
    content_h: f32,
}

impl Column {
    /// Whether the column has anything to show, and so takes space: not while
    /// switched off, nor while a fullscreen window has the keyboard if that is
    /// to hold them back (they wait, and show when it's left).
    fn showing(&self) -> bool {
        self.enabled
            && !(self.hide_in_fullscreen && self.fullscreen.load(Ordering::Relaxed))
            && !self.shared.store().is_empty()
    }

    /// The config's changes, and the notifications whose time has run out.
    fn tick(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        if self.config_checked.is_none_or(|t| t + HEARTBEAT <= now) {
            self.config_checked = Some(now);
            if let Some(config) = config::reload(&mut self.config_seen) {
                self.enabled = config.enabled;
                self.hide_in_fullscreen = config.hide_in_fullscreen;
                self.shared.enabled.store(config.enabled, Ordering::Relaxed);
                if !config.enabled {
                    let ids: Vec<u32> = self.shared.store().notes().iter().map(|n| n.id).collect();
                    for id in ids {
                        self.shared.store().close(id);
                        self.daemon.closed(id, store::DISMISSED);
                    }
                }
            }
        }
        let gone = self.shared.store().expire(now);
        for id in gone {
            self.daemon.closed(id, store::EXPIRED);
        }
        let mut wait = HEARTBEAT;
        if let Some(t) = self.shared.store().next_expiry() {
            wait = wait.min(t.saturating_duration_since(now));
        }
        ctx.request_repaint_after(wait);
    }
}

impl Shell for Column {
    fn setup(&mut self, ctx: &egui::Context) {
        self.skin.apply(ctx);
        if self.skin.follows_desktop() {
            patina::appearance::watch(ctx);
        }
    }

    fn ui(&mut self, ui: &mut Ui, _output: &str) {
        self.tick(ui.ctx());
        let rect = ui.max_rect();
        self.surface_h = rect.height();
        let notes = if self.showing() {
            self.shared.store().notes().to_vec()
        } else {
            Vec::new()
        };
        if notes.is_empty() {
            self.content_h = 0.0;
            return;
        }
        let mut inner = ui.new_child(
            UiBuilder::new()
                .max_rect(rect.shrink(PAD))
                .layout(Layout::top_down(Align::Min)),
        );
        let mut done: Vec<(u32, Outcome)> = Vec::new();
        let out = ScrollArea::vertical()
            .auto_shrink([false, true])
            .show(&mut inner, |ui| {
                for note in &notes {
                    let outcome = card(ui, note);
                    if outcome != Outcome::Nothing {
                        done.push((note.id, outcome));
                    }
                    ui.add_space(GAP);
                }
            });
        self.content_h = out.inner_rect.height() + 2.0 * PAD;
        for (id, outcome) in done {
            // A card goes when something happened: it was dismissed, a button
            // ran, or its app's window came forward. A click that did nothing
            // leaves it where it is.
            let gone = match &outcome {
                Outcome::Nothing => false,
                Outcome::Dismiss => true,
                Outcome::Action(key) => {
                    self.daemon.action(id, key);
                    true
                }
                Outcome::Open => {
                    let note = self
                        .shared
                        .store()
                        .notes()
                        .iter()
                        .find(|n| n.id == id)
                        .cloned();
                    note.is_some_and(|note| {
                        if note.actions.iter().any(|(k, _)| k == "default") {
                            self.daemon.action(id, "default");
                        }
                        let window = store::window_for(&note, &self.windows);
                        if let Some(window) = window {
                            self.requests.push(ToplevelRequest::Activate(window));
                        }
                        window.is_some()
                    })
                }
            };
            if gone && self.shared.store().close(id).is_some() {
                self.daemon.closed(id, store::DISMISSED);
            }
        }
    }

    fn toplevels_changed(&mut self, toplevels: &[Toplevel]) {
        self.windows = toplevels.iter().map(|t| (t.id, t.app_id.clone())).collect();
    }

    fn take_toplevel_requests(&mut self) -> Vec<ToplevelRequest> {
        std::mem::take(&mut self.requests)
    }

    fn wanted_size(&self) -> Option<(u32, u32)> {
        Some((if self.showing() { WIDTH } else { 1 }, 0))
    }

    fn wanted_exclusive_zone(&self) -> Option<i32> {
        Some(if self.showing() { WIDTH as i32 } else { 0 })
    }

    fn wanted_input_trim(&self) -> Option<(i32, i32, i32, i32)> {
        let below = (self.surface_h - self.content_h).max(0.0);
        Some((0, 0, below as i32, 0))
    }
}

/// One card; what was done to it, if anything.
fn card(ui: &mut Ui, note: &Note) -> Outcome {
    let mut outcome = Outcome::Nothing;
    let visuals = ui.visuals().clone();
    let edge = match note.urgency {
        Urgency::Critical => visuals.error_fg_color,
        _ => visuals.widgets.noninteractive.bg_stroke.color,
    };
    let card = ui.scope_builder(UiBuilder::new().sense(Sense::click()), |ui| {
        Frame::new()
            .fill(visuals.panel_fill)
            .stroke(Stroke::new(1.0, edge))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(Margin::same(12))
            .show(ui, |ui| {
                ui.set_width(CARD - 24.0);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(&note.app)
                            .size(12.0)
                            .color(visuals.weak_text_color()),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if close_button(ui) {
                            outcome = Outcome::Dismiss;
                        }
                    });
                });
                if !note.summary.is_empty() {
                    ui.add(
                        egui::Label::new(RichText::new(&note.summary).size(15.0).strong()).wrap(),
                    );
                }
                if !note.body.is_empty() {
                    ui.add(egui::Label::new(RichText::new(&note.body).size(13.0)).wrap());
                }
                let buttons: Vec<_> = note
                    .actions
                    .iter()
                    .filter(|(k, _)| k != "default")
                    .collect();
                if !buttons.is_empty() {
                    ui.add_space(4.0);
                    ui.horizontal_wrapped(|ui| {
                        for (key, label) in buttons {
                            if ui.button(label).clicked() {
                                outcome = Outcome::Action(key.clone());
                            }
                        }
                    });
                }
            });
    });
    if outcome == Outcome::Nothing {
        if card.response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if card.response.clicked() {
            outcome = Outcome::Open;
        }
    }
    outcome
}

/// The ✕ in a card's top right: two strokes, as the dialogs draw theirs, so no
/// font has to have the glyph.
fn close_button(ui: &mut Ui) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
    let colour = if response.hovered() {
        ui.visuals().text_color()
    } else {
        ui.visuals().weak_text_color()
    };
    let (centre, d) = (rect.center(), 4.5);
    let stroke = Stroke::new(1.5, colour);
    ui.painter()
        .line_segment([centre + vec2(-d, -d), centre + vec2(d, d)], stroke);
    ui.painter()
        .line_segment([centre + vec2(-d, d), centre + vec2(d, -d)], stroke);
    response.clicked()
}

pub fn run() -> Result<(), String> {
    let shared = Shared::new();
    let (wake, woken) = channel::channel::<()>();
    let fullscreen = Arc::new(AtomicBool::new(false));
    fullscreen::watch(fullscreen.clone(), wake.clone());
    let daemon = Daemon::serve(shared.clone(), wake)?;
    let column = Column {
        skin: piece::skin("patina-notify"),
        shared,
        daemon,
        windows: Vec::new(),
        requests: Vec::new(),
        enabled: true,
        hide_in_fullscreen: true,
        fullscreen,
        config_seen: None,
        config_checked: None,
        surface_h: 0.0,
        content_h: 0.0,
    };
    let options = Options {
        namespace: "patina-notify".into(),
        layer: Layer::Top,
        anchor: Anchor::TOP | Anchor::RIGHT | Anchor::BOTTOM,
        size: (1, 0),
        exclusive_zone: 0,
        margin: (0, 0, 0, 0),
        keyboard: KeyboardInteractivity::None,
        per_output: false,
        toplevels: true,
        click_through: false,
        lock: false,
        window_fallback: false,
    };
    let (host, event_loop) = Host::connect(options, column)?;
    event_loop
        .handle()
        .insert_source(woken, |event, _, host| {
            if let channel::Event::Msg(()) = event {
                host.redraw_all();
            }
        })
        .map_err(|e| e.to_string())?;
    host.run(event_loop)
}
