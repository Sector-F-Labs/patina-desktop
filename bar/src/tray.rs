//! The tray: the bar's status icons. Wi-Fi, showing the signal, opens the
//! Wi-Fi menu (patina-wifi, ADR-0022); the speaker, showing the level and
//! mute, opens the volume menu (patina-volume), which also flashes up when
//! the volume keys are used (ADR-0021); the battery shows the charge
//! (ADR-0024). The power button, after the clock, opens the power menu
//! (patina-power, ADR-0048).

use egui::{Sense, Ui, vec2};
use patina_system::audio::Volume;

/// Space around the icon inside its button, as the launcher's.
const PADDING: f32 = 10.0;

/// The Wi-Fi icon; runs `command` when it's clicked. `signal` is `None`
/// without a Wi-Fi device (then there's no icon), `Some(None)` when off or
/// not connected.
pub fn wifi(ui: &mut Ui, signal: Option<Option<u8>>, command: &str, skin: &str, edge: &str) {
    let Some(strength) = signal else { return };
    let side = ui.available_height();
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
    patina_system::icons::wifi(ui.painter(), rect.shrink(PADDING), strength, colour);
    let tip = match strength {
        Some(s) => format!("Wi-Fi {s}%"),
        None => "Wi-Fi not connected".into(),
    };
    if response.on_hover_text(tip).clicked() {
        crate::launcher::run(command, skin, edge);
    }
}

/// The power button; runs `command`, the power menu, when it's clicked.
pub fn power(ui: &mut Ui, command: &str, skin: &str, edge: &str) {
    let side = ui.available_height();
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
    patina_system::icons::power(ui.painter(), rect.shrink(PADDING), colour);
    if response
        .on_hover_text("Lock, log out, restart or shut down")
        .clicked()
    {
        crate::launcher::run(command, skin, edge);
    }
}

/// The speaker; runs `command` when it's clicked. Nothing without a sound
/// output.
pub fn speaker(ui: &mut Ui, volume: Option<Volume>, command: &str, skin: &str, edge: &str) {
    let Some(volume) = volume else { return };
    let side = ui.available_height();
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
        rect.shrink(PADDING),
        volume.level,
        volume.muted,
        colour,
    );
    let tip = if volume.muted {
        "Muted".to_string()
    } else {
        format!("Volume {}%", (volume.level * 100.0).round() as i32)
    };
    let response = response.on_hover_text(tip);
    if response.clicked() {
        crate::launcher::run(command, skin, edge);
    }
}

/// The battery and its percentage, with the time left on hover; nothing
/// on a machine without one. At 10% or under, while discharging, it's in
/// the error colour.
pub fn battery(ui: &mut Ui, battery: Option<patina_system::power::Battery>) {
    let Some(b) = battery else { return };
    let v = ui.visuals().clone();
    let colour = if b.low() {
        v.error_fg_color
    } else {
        v.text_color()
    };
    let charging = b.charge == patina_system::power::Charge::Charging;
    let text = format!("{}%", b.percent);
    let font = egui::TextStyle::Body.resolve(ui.style());
    let galley = ui.painter().layout_no_wrap(text, font, colour);
    let side = ui.available_height();
    let size = vec2(side * 0.7 + galley.size().x + 6.0, side);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    let icon = egui::Rect::from_min_size(rect.min, vec2(side * 0.7, side));
    patina_system::icons::battery(
        ui.painter(),
        icon.shrink2(vec2(2.0, side * 0.33)),
        b.percent,
        charging,
        colour,
        v.panel_fill,
    );
    ui.painter().galley(
        egui::pos2(icon.right() + 4.0, rect.center().y - galley.size().y / 2.0),
        galley,
        colour,
    );
    response.on_hover_text(b.describe());
}
