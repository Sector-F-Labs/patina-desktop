//! A tooltip (ADR-0057), for a piece too small to show its own: the bar
//! starts one past its edge while something on it is hovered. It takes no
//! clicks or keys, and closes when its standard input does, so it never
//! outlives the hover, or the program that started it.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use egui::{Align2, Area, Frame, Id, Order, Ui};
use patina::skin::Skin;
use patina_shell::Shell;

use crate::overlay::{Edge, Place};

/// How often it looks to see if its input has closed.
const POLL: Duration = Duration::from_millis(100);
/// The widest a tooltip gets before its text wraps.
const WIDTH: f32 = 360.0;

pub struct Tooltip {
    place: Place,
    text: String,
    skin: Skin,
    /// Set when standard input closes.
    closed: Arc<AtomicBool>,
}

impl Tooltip {
    /// A tooltip saying `text` at `place`, until standard input closes.
    pub fn new(place: Place, text: String) -> Self {
        let closed = Arc::new(AtomicBool::new(false));
        let flag = closed.clone();
        std::thread::spawn(move || {
            let _ = std::io::copy(&mut std::io::stdin(), &mut std::io::sink());
            flag.store(true, Ordering::Relaxed);
        });
        Self::with_flag(place, text, closed)
    }

    fn with_flag(place: Place, text: String, closed: Arc<AtomicBool>) -> Self {
        Self {
            place,
            text,
            skin: crate::bar_skin(),
            closed,
        }
    }
}

impl Shell for Tooltip {
    fn setup(&mut self, ctx: &egui::Context) {
        self.skin.apply(ctx);
        patina::appearance::watch(ctx);
    }

    fn ui(&mut self, ui: &mut Ui, _: &str) {
        let ctx = ui.ctx().clone();
        let screen = ctx.content_rect();
        let (pivot, pos) = match self.place.edge {
            Edge::Top => (Align2::CENTER_TOP, self.place.at),
            Edge::Bottom => (
                Align2::CENTER_BOTTOM,
                egui::pos2(self.place.at.x, screen.bottom() - self.place.at.y),
            ),
            Edge::Left => (Align2::LEFT_CENTER, self.place.at),
            Edge::Right => (
                Align2::RIGHT_CENTER,
                egui::pos2(screen.right() - self.place.at.x, self.place.at.y),
            ),
        };
        Area::new(Id::new("tooltip"))
            .order(Order::Tooltip)
            .pivot(pivot)
            .fixed_pos(pos)
            .constrain(true)
            .interactable(false)
            .show(&ctx, |ui| {
                Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_max_width(WIDTH);
                    ui.label(&self.text);
                })
            });
        ctx.request_repaint_after(POLL);
    }

    fn finished(&self) -> bool {
        self.closed.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Pos2, pos2};

    #[test]
    fn it_sits_centred_past_the_edge_and_closes_with_its_input() {
        let closed = Arc::new(AtomicBool::new(false));
        let place = Place {
            at: pos2(400.0, 50.0),
            edge: Edge::Bottom,
        };
        let mut tip = Tooltip::with_flag(place, "Wi-Fi 72%".into(), closed.clone());
        let ctx = egui::Context::default();
        for _ in 0..2 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    Pos2::ZERO,
                    egui::vec2(1600.0, 1000.0),
                )),
                ..Default::default()
            };
            let _ = ctx.run_ui(input, |ui| tip.ui(ui, ""));
        }
        let rect = ctx.memory(|m| m.area_rect(Id::new("tooltip"))).unwrap();
        assert_eq!(rect.bottom(), 950.0);
        assert!((rect.center().x - 400.0).abs() < 1.0, "{rect:?}");
        assert!(!tip.finished());
        closed.store(true, Ordering::Relaxed);
        assert!(tip.finished());
    }
}
