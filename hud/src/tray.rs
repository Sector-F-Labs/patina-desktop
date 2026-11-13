//! The tray: the bar's status icons. Wi-Fi, showing the signal, opens the
//! Wi-Fi menu (patina-wifi, ADR-0022); the speaker, showing the level and
//! mute, opens the volume menu (patina-volume), which also flashes up when
//! the volume keys are used (ADR-0021); the battery shows the charge
//! (ADR-0024) and opens Settings' Power and Battery page. The keyboard layout in use shows when there's more than one
//! (ADR-0068). The power button, after the clock, opens the power menu
//! (patina-power, ADR-0048).

use egui::{Sense, Ui, vec2};
use patina_system::audio::Volume;

/// Space around the icon inside a button `side` wide: 10 px, as the
/// launcher's, in a bar's height, less in a thin tray so its icons stay a
/// size to see (16 px at 24).
pub fn padding(side: f32) -> f32 {
    ((side - 16.0) / 2.0).clamp(2.0, 10.0)
}

/// The side of a square button in `ui`: as thick as the bar, across a row
/// or a column, whatever room is left along it.
pub fn side(ui: &Ui) -> f32 {
    if ui.layout().main_dir().is_vertical() {
        ui.available_width()
    } else {
        ui.available_height()
    }
}

/// An app's status icon (ADR-0064): its image at the button's icon size
/// on whole pixels, or its name's first letter while there's none; with
/// `tip` on hover. The bar does what its clicks ask.
pub fn status_item(
    ui: &mut Ui,
    texture: Option<egui::TextureId>,
    name: &str,
    tip: &str,
) -> (egui::Rect, egui::Response) {
    let side = side(ui);
    let (rect, response) = ui.allocate_exact_size(vec2(side, side), Sense::click());
    let v = ui.visuals();
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, 0.0, v.widgets.hovered.weak_bg_fill);
    }
    let icon = (side - 2.0 * padding(side)).round();
    let corner = rect.center() - vec2(icon, icon) / 2.0;
    let corner = egui::emath::GuiRounding::round_to_pixels(corner, ui.ctx().pixels_per_point());
    let mark = egui::Rect::from_min_size(corner, vec2(icon, icon));
    match texture {
        Some(texture) => {
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            ui.painter().image(texture, mark, uv, egui::Color32::WHITE);
        }
        None => {
            let letter = name
                .chars()
                .next()
                .map(|c| c.to_uppercase().to_string())
                .unwrap_or_default();
            ui.painter()
                .rect_filled(mark, 4.0, v.widgets.inactive.bg_fill);
            ui.painter().text(
                mark.center(),
                egui::Align2::CENTER_CENTER,
                letter,
                egui::TextStyle::Small.resolve(ui.style()),
                v.text_color(),
            );
        }
    }
    crate::tooltip::hover(ui, &response, tip);
    (rect, response)
}

/// A square button drawing `paint` in the skin's text colour, with `tip` on
/// hover.
fn icon_button(
    ui: &mut Ui,
    paint: impl FnOnce(&egui::Painter, egui::Rect, egui::Color32),
    tip: &str,
) -> egui::Response {
    let side = side(ui);
    let (rect, response) = ui.allocate_exact_size(vec2(side, side), Sense::click());
    let v = ui.visuals();
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, 0.0, v.widgets.hovered.weak_bg_fill);
    }
    let colour = if response.hovered() {
        v.strong_text_color()
    } else {
        v.text_color()
    };
    // The skip buttons' glyphs are smaller than a status icon's.
    paint(
        ui.painter(),
        rect.shrink(padding(rect.width()) + 2.0),
        colour,
    );
    crate::tooltip::hover(ui, &response, tip);
    response
}

/// A small count in the corner of `rect`, in `colour` with the number cut out of it in
/// `ink`: how many players there are (ADR-0117). Ten or more show as 9+.
fn badge(
    painter: &egui::Painter,
    rect: egui::Rect,
    count: usize,
    colour: egui::Color32,
    ink: egui::Color32,
) {
    // Inside the button: it fills the bar's thickness, so anything outside is cut off.
    let radius = (rect.width().min(rect.height()) * 0.22).clamp(5.0, 8.0);
    let centre = egui::pos2(rect.right() - radius - 1.0, rect.bottom() - radius - 1.0);
    painter.circle_filled(centre, radius, colour);
    let text = if count > 9 {
        "9+".to_string()
    } else {
        count.to_string()
    };
    painter.text(
        centre,
        egui::Align2::CENTER_CENTER,
        text,
        egui::FontId::proportional(radius * 1.35),
        ink,
    );
}

/// With more than one player the controls are one button, for the one shown, with their
/// count; it opens the volume menu, which has a row of controls for each (ADR-0117).
pub fn media_many(
    ui: &mut Ui,
    player: &patina_system::mpris::Player,
    count: usize,
    command: &str,
    skin: &str,
    opening: &dyn Fn(egui::Rect) -> crate::launcher::Opening,
) {
    use patina_system::icons;
    use patina_system::mpris::Status;
    let playing = player.status == Status::Playing;
    let tip = format!("{count} players\n{}", player.describe());
    let side = side(ui);
    let (rect, response) = ui.allocate_exact_size(vec2(side, side), Sense::click());
    let v = ui.visuals();
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, 0.0, v.widgets.hovered.weak_bg_fill);
    }
    let colour = if response.hovered() {
        v.strong_text_color()
    } else {
        v.text_color()
    };
    let icon = rect.shrink(padding(rect.width()) + 2.0);
    if playing {
        icons::pause(ui.painter(), icon, colour);
    } else {
        icons::play(ui.painter(), icon, colour);
    }
    badge(ui.painter(), rect, count, colour, v.panel_fill);
    crate::tooltip::hover(ui, &response, tip);
    if response.clicked() {
        crate::launcher::run(command, skin, &opening(rect));
    }
}

/// The media controls (ADR-0101): previous, play or pause, and next, in a
/// row or a column as the bar is, with the track on hover. Returns what
/// was clicked.
pub fn media(
    ui: &mut Ui,
    player: &patina_system::mpris::Player,
) -> Option<patina_system::mpris::Command> {
    use patina_system::icons;
    use patina_system::mpris::{Command, Status};
    let mut clicked = None;
    if player.can_previous
        && icon_button(ui, |p, r, c| icons::skip(p, r, false, c), "Previous").clicked()
    {
        clicked = Some(Command::Previous);
    }
    let playing = player.status == Status::Playing;
    let tip = player.describe();
    let toggle = icon_button(
        ui,
        |p, r, c| {
            if playing {
                icons::pause(p, r, c)
            } else {
                icons::play(p, r, c)
            }
        },
        &tip,
    );
    if toggle.clicked() {
        clicked = Some(Command::PlayPause);
    }
    if player.can_next && icon_button(ui, |p, r, c| icons::skip(p, r, true, c), "Next").clicked() {
        clicked = Some(Command::Next);
    }
    clicked
}

/// The Wi-Fi icon; runs `command` when it's clicked. `signal` is `None`
/// without a Wi-Fi device (then there's no icon), `Some(None)` when off or
/// not connected.
pub fn wifi(
    ui: &mut Ui,
    signal: Option<Option<u8>>,
    command: &str,
    skin: &str,
    opening: &dyn Fn(egui::Rect) -> crate::launcher::Opening,
) {
    let Some(strength) = signal else { return };
    let side = side(ui);
    let (rect, response) = ui.allocate_exact_size(vec2(side, side), Sense::click());
    let v = ui.visuals();
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, 0.0, v.widgets.hovered.weak_bg_fill);
    }
    let colour = if response.hovered() {
        v.strong_text_color()
    } else {
        v.text_color()
    };
    patina_system::icons::wifi(
        ui.painter(),
        rect.shrink(padding(rect.width())),
        strength,
        colour,
    );
    let tip = match strength {
        Some(s) => format!("Wi-Fi {s}%"),
        None => "Wi-Fi not connected".into(),
    };
    crate::tooltip::hover(ui, &response, tip);
    if response.clicked() {
        crate::launcher::run(command, skin, &opening(rect));
    }
}

/// The clipboard icon; runs `command`, the history's menu, when it's clicked.
pub fn clipboard(
    ui: &mut Ui,
    command: &str,
    skin: &str,
    opening: &dyn Fn(egui::Rect) -> crate::launcher::Opening,
) {
    let side = side(ui);
    let (rect, response) = ui.allocate_exact_size(vec2(side, side), Sense::click());
    let v = ui.visuals();
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, 0.0, v.widgets.hovered.weak_bg_fill);
    }
    let colour = if response.hovered() {
        v.strong_text_color()
    } else {
        v.text_color()
    };
    patina_system::icons::clipboard(ui.painter(), rect.shrink(padding(rect.width())), colour);
    crate::tooltip::hover(ui, &response, "Clipboard history");
    if response.clicked() {
        crate::launcher::run(command, skin, &opening(rect));
    }
}

/// The coffee cup: steaming while the screen is held on; clicking it
/// holds the screen on or lets go. True when it was clicked.
pub fn caffeine(ui: &mut Ui, on: bool) -> bool {
    let side = side(ui);
    let (rect, response) = ui.allocate_exact_size(vec2(side, side), Sense::click());
    let v = ui.visuals();
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, 0.0, v.widgets.hovered.weak_bg_fill);
    }
    let colour = if response.hovered() {
        v.strong_text_color()
    } else {
        v.text_color()
    };
    patina_system::icons::coffee(ui.painter(), rect.shrink(padding(rect.width())), on, colour);
    let tip = if on {
        "Screen kept on\nClick to let it turn off"
    } else {
        "Click to keep the screen on"
    };
    crate::tooltip::hover(ui, &response, tip);
    response.clicked()
}

/// The keyboard layout in use, as "US", when there's more than one to
/// switch between; its button and how it was clicked.
pub fn layout(
    ui: &mut Ui,
    status: &patina_system::keyboard::Status,
) -> Option<(egui::Rect, egui::Response)> {
    if status.layouts.len() < 2 {
        return None;
    }
    let layout = status.current()?;
    let side = side(ui);
    let (rect, response) = ui.allocate_exact_size(vec2(side, side), Sense::click());
    let v = ui.visuals();
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, 0.0, v.widgets.hovered.weak_bg_fill);
    }
    let colour = if response.hovered() {
        v.strong_text_color()
    } else {
        v.text_color()
    };
    let font = egui::TextStyle::Body.resolve(ui.style());
    // A rune the skin's font hasn't got would be a box.
    let label = if ui.ctx().fonts_mut(|f| f.has_glyphs(&font, &layout.label)) {
        layout.label.clone()
    } else {
        patina_system::keyboard::plain_label(&layout.code)
    };
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        font,
        colour,
    );
    crate::tooltip::hover(
        ui,
        &response,
        format!("{}\nCtrl+Space for the next", layout.name),
    );
    Some((rect, response))
}

/// The clock showing `text`; runs `command`, its menu, when it's clicked,
/// with the date in full on hover.
pub fn clock(
    ui: &mut Ui,
    text: String,
    command: &str,
    skin: &str,
    opening: &dyn Fn(egui::Rect) -> crate::launcher::Opening,
) {
    let v = ui.visuals().clone();
    let font = egui::TextStyle::Body.resolve(ui.style());
    let galley = ui
        .painter()
        .layout_no_wrap(text, font, egui::Color32::PLACEHOLDER);
    let side = side(ui);
    let size = if ui.layout().main_dir().is_vertical() {
        vec2(side, galley.size().y + 2.0 * padding(side))
    } else {
        vec2(galley.size().x + 2.0 * padding(side), side)
    };
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, 0.0, v.widgets.hovered.weak_bg_fill);
    }
    let colour = if response.hovered() {
        v.strong_text_color()
    } else {
        v.text_color()
    };
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, colour);
    crate::tooltip::hover(
        ui,
        &response,
        chrono::Local::now().format("%A %-d %B %Y").to_string(),
    );
    if response.clicked() {
        crate::launcher::run(command, skin, &opening(rect));
    }
}

/// The power button; runs `command`, the power menu, when it's clicked.
pub fn power(
    ui: &mut Ui,
    command: &str,
    skin: &str,
    opening: &dyn Fn(egui::Rect) -> crate::launcher::Opening,
) {
    let side = side(ui);
    let (rect, response) = ui.allocate_exact_size(vec2(side, side), Sense::click());
    let v = ui.visuals();
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, 0.0, v.widgets.hovered.weak_bg_fill);
    }
    let colour = if response.hovered() {
        v.strong_text_color()
    } else {
        v.text_color()
    };
    patina_system::icons::power(ui.painter(), rect.shrink(padding(rect.width())), colour);
    crate::tooltip::hover(ui, &response, "Lock, log out, restart or shut down");
    if response.clicked() {
        crate::launcher::run(command, skin, &opening(rect));
    }
}

/// The speaker; runs `command` when it's clicked. Nothing without a sound
/// output.
pub fn speaker(
    ui: &mut Ui,
    volume: Option<Volume>,
    command: &str,
    skin: &str,
    opening: &dyn Fn(egui::Rect) -> crate::launcher::Opening,
) {
    let Some(volume) = volume else { return };
    let side = side(ui);
    let (rect, response) = ui.allocate_exact_size(vec2(side, side), Sense::click());
    let v = ui.visuals();
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, 0.0, v.widgets.hovered.weak_bg_fill);
    }
    let colour = if response.hovered() {
        v.strong_text_color()
    } else {
        v.text_color()
    };
    patina_system::icons::speaker(
        ui.painter(),
        rect.shrink(padding(rect.width())),
        volume.level,
        volume.muted,
        colour,
    );
    let tip = if volume.muted {
        "Muted".to_string()
    } else {
        format!("Volume {}%", (volume.level * 100.0).round() as i32)
    };
    crate::tooltip::hover(ui, &response, tip);
    if response.clicked() {
        crate::launcher::run(command, skin, &opening(rect));
    }
}

/// The battery and its percentage, with the time left on hover; nothing
/// on a machine without one. At 10% or under, while discharging, it's in
/// the error colour. Runs `command` when it's clicked.
pub fn battery(
    ui: &mut Ui,
    battery: Option<patina_system::power::Battery>,
    command: &str,
    skin: &str,
    opening: &dyn Fn(egui::Rect) -> crate::launcher::Opening,
) {
    let Some(b) = battery else { return };
    let v = ui.visuals().clone();
    let charging = b.charge == patina_system::power::Charge::Charging;
    let text = format!("{}%", b.percent);
    let font = egui::TextStyle::Body.resolve(ui.style());
    // Coloured when painted, once it's known whether it's hovered.
    let galley = ui
        .painter()
        .layout_no_wrap(text, font, egui::Color32::PLACEHOLDER);
    let side = side(ui);
    // In a column there's no room for the percentage: it's on hover.
    let column = ui.layout().main_dir().is_vertical();
    let size = if column {
        vec2(side, side)
    } else {
        vec2(side * 0.7 + galley.size().x + 6.0, side)
    };
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let background = if response.hovered() {
        ui.painter()
            .rect_filled(rect, 0.0, v.widgets.hovered.weak_bg_fill);
        v.widgets.hovered.weak_bg_fill
    } else {
        v.panel_fill
    };
    let colour = if b.low() {
        v.error_fg_color
    } else if response.hovered() {
        v.strong_text_color()
    } else {
        v.text_color()
    };
    let icon = if column {
        egui::Rect::from_center_size(rect.center(), vec2(side * 0.7, side))
    } else {
        egui::Rect::from_min_size(rect.min, vec2(side * 0.7, side))
    };
    patina_system::icons::battery(
        ui.painter(),
        icon.shrink2(vec2(2.0, side * 0.33)),
        b.percent,
        charging,
        colour,
        background,
    );
    if !column {
        ui.painter().galley(
            egui::pos2(icon.right() + 4.0, rect.center().y - galley.size().y / 2.0),
            galley,
            colour,
        );
    }
    crate::tooltip::hover(ui, &response, b.describe());
    if response.clicked() {
        crate::launcher::run(command, skin, &opening(rect));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `side` in a `w` by `h` space laid out by `layout`.
    fn side_in(w: f32, h: f32, layout: egui::Layout) -> f32 {
        let ctx = egui::Context::default();
        let mut found = 0.0;
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), vec2(w, h));
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(layout));
            found = side(&child);
            // Past the room left in the row, as the last button of a
            // cramped tray is.
            child.allocate_space(vec2(w + 20.0, 1.0));
            found = found.min(side(&child));
        });
        found
    }

    /// A button at the end of a row with no room left once drew tiny, or
    /// flipped (a negative size): it's as thick as the bar regardless.
    #[test]
    fn a_button_is_as_thick_as_the_bar_whatever_room_is_left() {
        assert_eq!(
            side_in(30.0, 41.0, egui::Layout::left_to_right(egui::Align::Center)),
            41.0
        );
        assert_eq!(
            side_in(41.0, 30.0, egui::Layout::top_down(egui::Align::Center)),
            41.0
        );
    }
}
