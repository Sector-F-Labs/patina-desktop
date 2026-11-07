//! The Displays page (ADR-0121): the monitors drawn to scale, to drag to the
//! side of another they sit on; whether the desktop extends across them,
//! mirrors, or runs on the primary alone; and the monitors connected but
//! off, waiting to be chosen for. All of it through the compositor's
//! `com.sectorflabs.Patina.Displays`, which keeps the choice by monitor.

use std::sync::mpsc;
use std::time::Duration;

use egui::{
    Align2, CornerRadius, Pos2, Rect, RichText, Sense, Shape, Stroke, StrokeKind, TextStyle, Ui,
    Vec2, pos2, vec2,
};
use patina_system::displays::{self, Mode, Monitor, Saved, Side};

use crate::app::Message;
use crate::display_layout::{R, Snap, snap};

/// How often the compositor is asked which monitors there are.
const POLL: Duration = Duration::from_millis(500);
/// The canvas's height in points, and the room round the monitors in it, as a
/// share of the largest, for somewhere to drop one.
const CANVAS_HEIGHT: f32 = 260.0;
const ROOM: f64 = 0.3;

/// What the compositor said.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    monitors: Vec<Monitor>,
    mode: Mode,
    /// The displays remembered, connected or not (ADR-0124).
    saved: Vec<Saved>,
}

/// A monitor being dragged on the canvas.
struct Drag {
    id: String,
    /// Where in its tile it was picked up, in screen points.
    grab: Vec2,
}

pub struct DisplaysPage {
    /// `None` until the compositor has answered.
    state: Option<Result<Snapshot, String>>,
    updates: mpsc::Receiver<Result<Snapshot, String>>,
    drag: Option<Drag>,
}

impl DisplaysPage {
    pub fn new(ctx: &egui::Context) -> Self {
        Self {
            state: None,
            updates: watch(ctx.clone()),
            drag: None,
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, message: &mut Option<Message>) {
        while let Ok(state) = self.updates.try_recv() {
            self.state = Some(state);
        }
        ui.heading("Displays");
        ui.label("Where the monitors sit, and whether the desktop spreads across them.");
        ui.add_space(8.0);
        let snapshot = match &self.state {
            None => return,
            Some(Err(e)) => {
                ui.label(
                    RichText::new(format!(
                        "Displays are the Patina compositor's, running on the hardware: {e}"
                    ))
                    .weak(),
                );
                return;
            }
            Some(Ok(snapshot)) => snapshot.clone(),
        };
        let active: Vec<&Monitor> = snapshot.monitors.iter().filter(|m| m.active).collect();
        let waiting: Vec<&Monitor> = snapshot.monitors.iter().filter(|m| !m.active).collect();
        let report = |result: Result<(), String>, message: &mut Option<Message>| {
            if let Err(e) = result {
                *message = Some(Message::error(format!("Couldn't change the displays: {e}")));
            }
        };

        let new: Vec<&&Monitor> = active.iter().filter(|m| !m.known && !m.primary).collect();
        if !new.is_empty() && snapshot.mode == Mode::Extend {
            let names: Vec<_> = new.iter().map(|m| m.label()).collect();
            ui.label(
                RichText::new(format!(
                    "{} is new. Drag it to the side of another display it sits on.",
                    names.join(", ")
                ))
                .color(ui.visuals().selection.stroke.color),
            );
            ui.add_space(6.0);
        }

        if !waiting.is_empty() {
            for m in &waiting {
                ui.horizontal(|ui| {
                    ui.label(format!(
                        "{} ({}×{}) is connected but off.",
                        m.label(),
                        m.width,
                        m.height
                    ));
                    if ui.button("Extend onto it").clicked() {
                        report(displays::set_mode(Mode::Extend), message);
                    }
                    if ui.button("Mirror").clicked() {
                        report(displays::set_mode(Mode::Mirror), message);
                    }
                });
            }
            ui.add_space(8.0);
        }

        // The three ways to use them.
        let mut mode = snapshot.mode;
        ui.horizontal(|ui| {
            ui.radio_value(&mut mode, Mode::Extend, "Extend");
            ui.radio_value(&mut mode, Mode::Mirror, "Mirror");
            ui.radio_value(&mut mode, Mode::Off, "Primary only");
        });
        if mode != snapshot.mode {
            report(displays::set_mode(mode), message);
        }
        ui.add_space(8.0);

        if active.len() < 2 {
            ui.label(RichText::new("Only one display is in use.").weak());
        } else if snapshot.mode == Mode::Mirror {
            ui.label(RichText::new("Every display shows the primary's desktop.").weak());
        } else {
            self.canvas(ui, &active, message);
            ui.add_space(10.0);
            self.places(ui, &active, message);
        }
        ui.add_space(10.0);
        for m in &active {
            let primary = if m.primary { ", the primary" } else { "" };
            ui.label(
                RichText::new(format!(
                    "{} on {}: {}×{} at {}×{primary}",
                    m.label(),
                    m.name,
                    m.width,
                    m.height,
                    m.scale
                ))
                .small()
                .weak(),
            );
        }
        ui.add_space(14.0);
        saved_list(ui, &snapshot.saved, message);
    }

    /// The monitors to scale, the others dragged to a side of one.
    fn canvas(&mut self, ui: &mut Ui, active: &[&Monitor], message: &mut Option<Message>) {
        let rects: Vec<R> = active
            .iter()
            .map(|m| R::new(m.x, m.y, m.logical_width, m.logical_height))
            .collect();
        // The desktop's box, with room round it.
        let largest = rects.iter().map(|r| r.w.max(r.h)).max().unwrap_or(1) as f64;
        let room = largest * ROOM;
        let left = rects.iter().map(|r| r.x).min().unwrap_or(0) as f64 - room;
        let top = rects.iter().map(|r| r.y).min().unwrap_or(0) as f64 - room;
        let right = rects.iter().map(|r| r.x + r.w).max().unwrap_or(1) as f64 + room;
        let bottom = rects.iter().map(|r| r.y + r.h).max().unwrap_or(1) as f64 + room;
        let width = ui.available_width().min(680.0);
        let (area, _) = ui.allocate_exact_size(vec2(width, CANVAS_HEIGHT), Sense::hover());
        let scale = ((area.width() as f64 / (right - left))
            .min(area.height() as f64 / (bottom - top))) as f32;
        let shown = vec2((right - left) as f32 * scale, (bottom - top) as f32 * scale);
        let origin = area.center() - shown / 2.0;
        let to_screen = |x: f64, y: f64| {
            pos2(
                origin.x + (x - left) as f32 * scale,
                origin.y + (y - top) as f32 * scale,
            )
        };
        let tile = |r: &R| {
            Rect::from_min_size(
                to_screen(r.x as f64, r.y as f64),
                vec2(r.w as f32 * scale, r.h as f32 * scale),
            )
        };
        let v = ui.visuals().clone();
        ui.painter()
            .rect_filled(area, CornerRadius::same(8), v.extreme_bg_color);

        let pointer = ui.input(|i| i.pointer.interact_pos());
        let mut dropped: Option<(String, Snap, Vec<String>)> = None;
        let mut ghost: Option<Rect> = None;
        let mut moving: Option<(usize, Rect)> = None;
        for (i, m) in active.iter().enumerate() {
            let at = tile(&rects[i]);
            let id = ui.id().with(("monitor", &m.id));
            // The primary stays put: it's where the others are placed from.
            let sense = if m.primary {
                Sense::hover()
            } else {
                Sense::drag()
            };
            let response = ui.interact(at, id, sense);
            if response.drag_started()
                && let Some(p) = pointer
            {
                self.drag = Some(Drag {
                    id: m.id.clone(),
                    grab: p - at.min,
                });
            }
            let dragging = self.drag.as_ref().is_some_and(|d| d.id == m.id);
            if dragging && let (Some(p), Some(drag)) = (pointer, &self.drag) {
                let top_left = p - drag.grab;
                moving = Some((i, Rect::from_min_size(top_left, at.size())));
                // Where it would land: in desktop points, against the others.
                let desktop = R::new(
                    (left + ((top_left.x - origin.x) / scale) as f64) as i32,
                    (top + ((top_left.y - origin.y) / scale) as f64) as i32,
                    rects[i].w,
                    rects[i].h,
                );
                let others: Vec<R> = rects
                    .iter()
                    .enumerate()
                    .filter(|(j, _)| *j != i)
                    .map(|(_, r)| *r)
                    .collect();
                if let Some(s) = snap(desktop, &others) {
                    ghost = Some(tile(&s.place(desktop, &others)));
                    if response.drag_stopped() {
                        let ids = active
                            .iter()
                            .enumerate()
                            .filter(|(j, _)| *j != i)
                            .map(|(_, m)| m.id.clone())
                            .collect();
                        dropped = Some((m.id.clone(), s, ids));
                    }
                }
            }
            if response.drag_stopped() {
                self.drag = None;
            }
            if dragging {
                continue;
            }
            draw_tile(ui, at, m, response.hovered() && !m.primary);
        }
        if let Some(g) = ghost {
            let r = g.shrink(0.75);
            let corners = [
                r.left_top(),
                r.right_top(),
                r.right_bottom(),
                r.left_bottom(),
                r.left_top(),
            ];
            ui.painter().extend(Shape::dashed_line(
                &corners,
                Stroke::new(1.5, v.selection.stroke.color),
                4.0,
                3.0,
            ));
        }
        if let Some((i, at)) = moving {
            draw_tile(ui, at, active[i], true);
        }
        if let Some((id, s, others)) = dropped
            && let Err(e) = displays::set_place(&id, Some((&others[s.to], s.side, s.offset)))
        {
            *message = Some(Message::error(format!("Couldn't place the display: {e}")));
        }
        if self.drag.is_some() {
            ui.ctx().request_repaint();
        }
    }

    /// The same choice as the drag, with menus: which side of which display
    /// each of the others is on.
    fn places(&mut self, ui: &mut Ui, active: &[&Monitor], message: &mut Option<Message>) {
        for m in active.iter().filter(|m| !m.primary) {
            let others: Vec<&&Monitor> = active.iter().filter(|o| o.id != m.id).collect();
            let mut side = m.side.unwrap_or(Side::Right);
            let mut to = others
                .iter()
                .find(|o| o.id == m.to)
                .or_else(|| others.first())
                .map(|o| o.id.clone())
                .unwrap_or_default();
            let before = (side, to.clone());
            ui.horizontal(|ui| {
                ui.label(format!("{} is", m.label()));
                egui::ComboBox::from_id_salt(("side", &m.id))
                    .selected_text(side_label(side))
                    .show_ui(ui, |ui| {
                        for s in Side::ALL {
                            ui.selectable_value(&mut side, s, side_label(s));
                        }
                    });
                let name = |id: &str| {
                    others
                        .iter()
                        .find(|o| o.id == id)
                        .map_or_else(String::new, |o| o.label().to_string())
                };
                egui::ComboBox::from_id_salt(("to", &m.id))
                    .selected_text(name(&to))
                    .show_ui(ui, |ui| {
                        for o in &others {
                            ui.selectable_value(&mut to, o.id.clone(), o.label());
                        }
                    });
            });
            if (side, to.clone()) != before
                && let Err(e) = displays::set_place(&m.id, Some((&to, side, 0)))
            {
                *message = Some(Message::error(format!("Couldn't place the display: {e}")));
            }
        }
    }
}

/// The displays remembered: where each was put, whether or not it's
/// connected. A monitor of the same model plugged in later goes where one of
/// them was, so a hot desk's monitors are where they should be, and is
/// switched on without asking unless its entry says to ask (ADR-0126); one no
/// monitor goes by is grayed, and can be forgotten.
fn saved_list(ui: &mut Ui, saved: &[Saved], message: &mut Option<Message>) {
    ui.heading("Saved displays");
    ui.label(
        RichText::new(
            "Displays you place are remembered. A display of the same model plugged \
             in later goes where one was put, and is switched on without asking.",
        )
        .weak(),
    );
    ui.add_space(6.0);
    if saved.is_empty() {
        ui.label(RichText::new("None yet: drag a display to a side of another.").weak());
        return;
    }
    // Newest first: the one a new monitor of its model would go by is on top.
    for s in saved.iter().rev() {
        let mut forget = false;
        let mut ask = s.ask;
        ui.horizontal(|ui| {
            let name = RichText::new(&s.name);
            ui.label(if s.in_use { name.strong() } else { name.weak() });
            let where_ = match (s.side, s.to_name.is_empty()) {
                (Some(side), false) => format!("{} {}", side_label(side), s.to_name),
                _ => "not placed".to_string(),
            };
            let note = if s.in_use {
                format!("{where_}, in use")
            } else {
                where_
            };
            ui.label(RichText::new(note).small().weak());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                forget = ui.small_button("Forget").clicked();
                ui.checkbox(&mut ask, "Ask when plugged in");
            });
        });
        if ask != s.ask
            && let Err(e) = displays::set_ask(&s.id, ask)
        {
            *message = Some(Message::error(format!("Couldn't change the display: {e}")));
        }
        if forget && let Err(e) = displays::forget(&s.id) {
            *message = Some(Message::error(format!("Couldn't forget the display: {e}")));
        }
    }
}

fn side_label(side: Side) -> &'static str {
    match side {
        Side::Above => "above",
        Side::Below => "below",
        Side::Left => "left of",
        Side::Right => "right of",
    }
}

/// One monitor's tile: its name and size, a bar along the top of the
/// primary, a bright edge on one that's new or being moved.
fn draw_tile(ui: &Ui, at: Rect, m: &Monitor, lit: bool) {
    let v = ui.visuals();
    let painter = ui.painter();
    let new = !m.known && !m.primary;
    let edge = if lit || new {
        Stroke::new(2.0, v.selection.stroke.color)
    } else {
        v.window_stroke
    };
    painter.rect(
        at,
        CornerRadius::same(4),
        v.widgets.inactive.weak_bg_fill,
        edge,
        StrokeKind::Inside,
    );
    if m.primary {
        let bar = Rect::from_min_size(at.min + vec2(1.0, 1.0), vec2(at.width() - 2.0, 5.0));
        painter.rect_filled(
            bar,
            CornerRadius {
                nw: 3,
                ne: 3,
                sw: 0,
                se: 0,
            },
            v.text_color(),
        );
    }
    let font = TextStyle::Small.resolve(ui.style());
    let centre: Pos2 = at.center();
    painter.text(
        centre - vec2(0.0, 7.0),
        Align2::CENTER_CENTER,
        m.label(),
        font.clone(),
        v.text_color(),
    );
    painter.text(
        centre + vec2(0.0, 7.0),
        Align2::CENTER_CENTER,
        format!("{}×{}", m.width, m.height),
        font,
        v.weak_text_color(),
    );
}

/// Asks the compositor which monitors there are, on a thread, sending what
/// changes.
fn watch(ctx: egui::Context) -> mpsc::Receiver<Result<Snapshot, String>> {
    let (send, receive) = mpsc::channel();
    let _ = std::thread::Builder::new()
        .name("displays".into())
        .spawn(move || {
            let mut last = None;
            loop {
                let now = displays::list().and_then(|monitors| {
                    Ok(Snapshot {
                        monitors,
                        mode: displays::mode()?,
                        saved: displays::saved()?,
                    })
                });
                if last.as_ref() != Some(&now) {
                    if send.send(now.clone()).is_err() {
                        return;
                    }
                    ctx.request_repaint();
                    last = Some(now);
                }
                std::thread::sleep(POLL);
            }
        });
    receive
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor(name: &str, model: &str, w: i32, h: i32, at: (i32, i32), scale: f64) -> Monitor {
        Monitor {
            name: name.into(),
            id: name.into(),
            make: String::new(),
            model: model.into(),
            width: w,
            height: h,
            x: at.0,
            y: at.1,
            logical_width: (w as f64 / scale) as i32,
            logical_height: (h as f64 / scale) as i32,
            scale,
            primary: name == "eDP-1",
            known: name == "eDP-1",
            active: true,
            to: String::new(),
            side: None,
            offset: 0,
        }
    }

    fn saved(id: &str, name: &str, in_use: bool, to: &str, side: Side) -> Saved {
        Saved {
            id: id.into(),
            name: name.into(),
            in_use,
            ask: false,
            to: String::new(),
            to_name: to.into(),
            side: Some(side),
            offset: 0,
        }
    }

    /// Displays remembered: one in use, and two from other desks.
    fn saved_fixture() -> Vec<Saved> {
        vec![
            saved(
                "DEL-A0D3-HOME",
                "DELL U2720Q",
                false,
                "Built-in display",
                Side::Left,
            ),
            saved(
                "DEL-A0D3-WORK",
                "DELL U2720Q",
                false,
                "Built-in display",
                Side::Right,
            ),
            saved(
                "GSM-772E-AAA",
                "LG HDR WQHD",
                true,
                "Built-in display",
                Side::Above,
            ),
        ]
    }

    /// Draws the page in software with two monitors, a new one above the
    /// primary and a third waiting, as PNGs in the folder
    /// PATINA_SETTINGS_PREVIEW names, to look at.
    #[test]
    #[ignore]
    fn preview_the_page() {
        let dir = std::path::PathBuf::from(std::env::var("PATINA_SETTINGS_PREVIEW").unwrap());
        let (w, h) = (860usize, 600usize);
        let mut off = monitor("HDMI-A-1", "DELL U2720Q", 3840, 2160, (0, 0), 1.0);
        off.active = false;
        let cases = [
            (
                "displays-new",
                vec![
                    monitor("eDP-1", "8C44", 1920, 1200, (0, 1440), 1.2),
                    monitor("DP-3", "LG HDR WQHD", 3440, 1440, (0, 0), 1.0),
                ],
                Mode::Extend,
            ),
            (
                "displays-waiting",
                vec![monitor("eDP-1", "8C44", 1920, 1200, (0, 0), 1.2), off],
                Mode::Off,
            ),
        ];
        for (name, monitors, mode) in cases {
            let ctx = egui::Context::default();
            let (_, updates) = mpsc::channel();
            let mut page = DisplaysPage {
                state: Some(Ok(Snapshot {
                    monitors,
                    mode,
                    saved: saved_fixture(),
                })),
                updates,
                drag: None,
            };
            let mut message = None;
            let mut painter = patina_shell::raster::Painter::default();
            let mut pixels = vec![0u32; w * h];
            for _ in 0..3 {
                let input = egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        pos2(0.0, 0.0),
                        vec2(w as f32, h as f32),
                    )),
                    max_texture_side: Some(8192),
                    ..Default::default()
                };
                patina::skin::Skin::default_skin().apply(&ctx);
                let out = ctx.run_ui(input, |ui| {
                    egui::CentralPanel::default().show(ui, |ui| page.ui(ui, &mut message));
                });
                let primitives = ctx.tessellate(out.shapes, out.pixels_per_point);
                pixels.fill(0);
                painter.paint(
                    &mut pixels,
                    w,
                    h,
                    out.pixels_per_point,
                    &primitives,
                    &out.textures_delta,
                );
            }
            let rgba: Vec<u8> = pixels
                .iter()
                .flat_map(|p| {
                    let [b, g, r, a] = p.to_le_bytes();
                    [r, g, b, a]
                })
                .collect();
            image::save_buffer(
                dir.join(format!("{name}.png")),
                &rgba,
                w as u32,
                h as u32,
                image::ColorType::Rgba8,
            )
            .unwrap();
        }
    }
}
