//! The tray: the bar's status icons. For now the speaker, showing the
//! level and mute; a click opens the volume menu (patina-volume), which
//! also flashes up when the volume keys are used (ADR-0021).

use egui::{Sense, Ui, vec2};
use patina_shell::audio::Volume;

/// Space around the icon inside its button, as the launcher's.
const PADDING: f32 = 10.0;

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
    patina_shell::icons::speaker(
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
