//! The launcher button: the Sector F flask, which runs a command
//! (patina-launcher by default) when clicked. The logo is rasterised at the exact pixel size it
//! is drawn at, as a white mask, and tinted from the skin's text colour, so
//! skins reach it like everything else the bar paints.

use std::process::{Command, Stdio};

use egui::{Id, Sense, TextureHandle, TextureOptions, Ui, vec2};

/// Space around the logo inside a button `side` wide, in points: a Patina
/// header bar's margin (8), so the logo is as tall as a title bar's
/// contents, and a fifth of the button when that's smaller.
fn padding(side: f32) -> f32 {
    (side * 0.2).min(8.0)
}

/// Where a piece the bar opens goes: the bar's edge (`PATINA_BAR_EDGE`),
/// and where on the screen the button that opened it is
/// (`PATINA_OPEN_AT`, as patina-shell's `piece::opening` reads it), so it
/// opens from the button.
#[derive(Debug, Clone, PartialEq)]
pub struct Opening {
    pub edge: &'static str,
    pub at: Option<String>,
}

/// Draws the button; runs `command` when it's clicked, telling it the bar's
/// skin and where the button is, so the launcher matches and opens from it.
pub fn button(ui: &mut Ui, command: &str, skin: &str, opening: &dyn Fn(egui::Rect) -> Opening) {
    let side = crate::tray::side(ui);
    let (rect, response) = ui.allocate_exact_size(vec2(side, side), Sense::click());
    let v = ui.visuals();
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, 0.0, v.widgets.hovered.weak_bg_fill);
    }
    let tint = if response.hovered() {
        v.strong_text_color()
    } else {
        v.text_color()
    };
    let icon = rect.shrink(padding(side));
    let pixels = (icon.width() * ui.ctx().pixels_per_point())
        .round()
        .max(1.0) as u32;
    if let Some(texture) = texture(ui.ctx(), pixels) {
        ui.painter().image(
            texture.id(),
            icon,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            tint,
        );
    }
    if response.clicked() {
        run(command, skin, &opening(rect));
    }
}

/// The logo as a texture `pixels` wide, made once per size and context.
fn texture(ctx: &egui::Context, pixels: u32) -> Option<TextureHandle> {
    let id = Id::new(("sector-f-logo", pixels));
    if let Some(t) = ctx.data(|d| d.get_temp::<TextureHandle>(id)) {
        return Some(t);
    }
    let image = patina_system::logo::rasterise(pixels)?;
    let texture = ctx.load_texture("sector-f-logo", image, TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, texture.clone()));
    Some(texture)
}

/// Runs `command` through sh(1), telling it the bar's skin
/// (`PATINA_SKIN`), edge (`PATINA_BAR_EDGE`, `top` or `bottom`) and where
/// to open (`PATINA_OPEN_AT`). On a thread, so the bus never holds the bar
/// up: Patina's compositor starts it, so it's among the desktop's processes
/// (ADR-0045); under any other, or without a session bus, the bar does.
pub fn run(command: &str, skin: &str, opening: &Opening) {
    let (command, skin, opening) = (command.to_string(), skin.to_string(), opening.clone());
    std::thread::spawn(move || {
        let mut env = vec![
            ("PATINA_SKIN", skin.as_str()),
            ("PATINA_BAR_EDGE", opening.edge),
        ];
        if let Some(at) = &opening.at {
            env.push(("PATINA_OPEN_AT", at.as_str()));
        }
        if patina_system::processes::spawn(&command, &env).is_err() {
            run_here(&command, &env);
        }
    });
}

/// Runs `command` as the bar's own child, detached: no stdin, and reaped
/// so it never lingers as a zombie.
fn run_here(command: &str, env: &[(&str, &str)]) {
    match Command::new("sh")
        .arg("-c")
        .arg(command)
        .envs(env.iter().copied())
        .stdin(Stdio::null())
        .spawn()
    {
        Ok(mut child) => {
            let _ = child.wait();
        }
        Err(e) => eprintln!("patina-bar: can't run {command}: {e}"),
    }
}
