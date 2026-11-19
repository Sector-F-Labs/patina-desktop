//! The toolbox's symbols, painted with shapes so they take the skin's colours and are sharp at
//! any size. Each is drawn in a unit square (0..1 both ways) placed in `rect`.

use egui::epaint::PathStroke;
use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, Vec2};

use crate::document::Tool;

/// Paints `tool`'s symbol in `rect` (the middle of it, square) in `ink`.
pub fn paint(painter: &Painter, tool: Tool, rect: Rect, ink: Color32) {
    let side = rect.width().min(rect.height());
    let square = Rect::from_center_size(rect.center(), Vec2::splat(side));
    let at = |x: f32, y: f32| Pos2::new(square.left() + x * side, square.top() + y * side);
    let thin = Stroke::new((side * 0.075).max(1.2), ink);
    let thick = |width: f32| Stroke::new(side * width, ink);
    let round = |points: Vec<Pos2>, stroke: Stroke| Shape::line(points, stroke);
    let fill = |points: Vec<Pos2>| Shape::convex_polygon(points, ink, Stroke::NONE);
    match tool {
        Tool::Rectangle => {
            painter.rect_stroke(
                Rect::from_min_max(at(0.14, 0.24), at(0.86, 0.76)),
                side * 0.06,
                Stroke::new((side * 0.1).max(1.5), ink),
                egui::StrokeKind::Inside,
            );
        }
        Tool::Ellipse => {
            let points: Vec<Pos2> = (0..48)
                .map(|i| {
                    let a = i as f32 / 48.0 * std::f32::consts::TAU;
                    at(0.5 + 0.38 * a.cos(), 0.5 + 0.27 * a.sin())
                })
                .collect();
            painter.add(Shape::Path(egui::epaint::PathShape::closed_line(
                points,
                PathStroke::new((side * 0.1).max(1.5), ink),
            )));
        }
        Tool::Arrow => {
            painter.add(round(vec![at(0.16, 0.84), at(0.64, 0.36)], thick(0.1)));
            painter.add(fill(vec![at(0.86, 0.14), at(0.46, 0.28), at(0.72, 0.54)]));
        }
        Tool::Line => {
            painter.add(round(vec![at(0.14, 0.86), at(0.86, 0.14)], thick(0.1)));
        }
        Tool::Highlighter => {
            // The ink it leaves, behind a chisel-tipped marker.
            painter.add(round(vec![at(0.14, 0.88), at(0.58, 0.88)], Stroke::new(side * 0.13, ink.gamma_multiply(0.45))));
            painter.add(Shape::convex_polygon(
                vec![at(0.2, 0.68), at(0.62, 0.2), at(0.84, 0.42), at(0.42, 0.78)],
                Color32::TRANSPARENT,
                thin,
            ));
            painter.add(fill(vec![at(0.2, 0.68), at(0.42, 0.78), at(0.3, 0.84), at(0.14, 0.8)]));
        }
        Tool::Brush => {
            // A handle, its ferrule, and the bristles.
            painter.add(round(vec![at(0.86, 0.14), at(0.5, 0.5)], thick(0.1)));
            painter.add(round(vec![at(0.52, 0.48), at(0.4, 0.6)], thick(0.16)));
            painter.add(fill(vec![at(0.4, 0.58), at(0.5, 0.68), at(0.3, 0.88), at(0.12, 0.86), at(0.14, 0.68)]));
        }
        Tool::Pencil => {
            painter.add(round(vec![at(0.86, 0.14), at(0.34, 0.66)], thick(0.16)));
            painter.add(fill(vec![at(0.3, 0.6), at(0.4, 0.7), at(0.14, 0.86)]));
        }
        Tool::Eraser => {
            // A block tilted, its worn end filled, with the ground under it.
            painter.add(Shape::convex_polygon(
                vec![at(0.12, 0.6), at(0.5, 0.2), at(0.84, 0.5), at(0.46, 0.84)],
                Color32::TRANSPARENT,
                thin,
            ));
            painter.add(fill(vec![at(0.12, 0.6), at(0.32, 0.4), at(0.66, 0.68), at(0.46, 0.84)]));
            painter.add(round(vec![at(0.5, 0.9), at(0.9, 0.9)], thin));
        }
        Tool::Fill => {
            // A bucket tipped over, pouring a drop.
            painter.add(Shape::convex_polygon(
                vec![at(0.12, 0.46), at(0.42, 0.16), at(0.74, 0.48), at(0.44, 0.78)],
                Color32::TRANSPARENT,
                thin,
            ));
            painter.add(fill(vec![at(0.12, 0.46), at(0.28, 0.5), at(0.46, 0.66), at(0.44, 0.78)]));
            painter.circle_filled(at(0.84, 0.76), side * 0.09, ink);
            painter.add(fill(vec![at(0.84, 0.52), at(0.92, 0.7), at(0.76, 0.7)]));
        }
        Tool::Picker => {
            // A dropper: bulb, barrel and tip.
            painter.add(round(vec![at(0.74, 0.26), at(0.4, 0.6)], thick(0.13)));
            painter.circle_filled(at(0.78, 0.22), side * 0.12, ink);
            painter.add(round(vec![at(0.4, 0.6), at(0.16, 0.86)], thick(0.06)));
        }
    }
}
