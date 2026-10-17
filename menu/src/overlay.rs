//! What every menu here shares: it's drawn on a transparent surface over
//! the whole screen, so it can sit anywhere and stay on screen near the
//! edges, and a click anywhere else, Escape, or losing the keyboard closes
//! it.

use egui::{Align2, Area, Frame, Id, Order, Pos2, Ui};

/// Which screen edge `at`'s Y counts from, and so which way the menu opens:
/// down from a point (the top, or no edge), or up from it (the bottom, as
/// from a bar there).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Edge {
    #[default]
    Top,
    Bottom,
}

/// Where a menu opens.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Place {
    /// X from the screen's left, Y from `edge`, in logical pixels.
    pub at: Pos2,
    pub edge: Edge,
}

/// Whether the menu should close: it lost the keyboard after having it, or
/// Escape. `had_focus` remembers the first.
pub fn closing(ui: &Ui, had_focus: &mut bool) -> bool {
    if ui.input(|i| i.focused) {
        *had_focus = true;
    } else if *had_focus {
        return true;
    }
    ui.input(|i| i.key_pressed(egui::Key::Escape))
}

/// Draws the menu's items in a menu frame at `place`, kept on screen.
pub fn show(ui: &Ui, id: &str, place: Place, items: impl FnOnce(&mut Ui)) {
    let ctx = ui.ctx().clone();
    let screen = ctx.content_rect();
    let (pivot, pos) = match place.edge {
        Edge::Top => (Align2::LEFT_TOP, place.at),
        Edge::Bottom => (
            Align2::LEFT_BOTTOM,
            egui::pos2(place.at.x, screen.bottom() - place.at.y),
        ),
    };
    Area::new(Id::new(id))
        .order(Order::Foreground)
        .pivot(pivot)
        .fixed_pos(pos)
        .constrain(true)
        .show(&ctx, |ui| Frame::menu(ui.style()).show(ui, items));
}

/// Whether the pointer was pressed outside the menu and its submenus: on
/// the transparent background.
pub fn pressed_outside(ui: &Ui) -> bool {
    let ctx = ui.ctx().clone();
    // The layer is looked up after `input`, which holds the context's lock
    // that `layer_id_at` takes too.
    let pressed = ui.input(|i| {
        i.pointer
            .any_pressed()
            .then(|| i.pointer.interact_pos())
            .flatten()
    });
    pressed.is_some_and(|p| {
        ctx.layer_id_at(p)
            .is_none_or(|layer| layer.order == Order::Background)
    })
}
