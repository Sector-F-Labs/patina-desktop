//! patina-settings: the Patina desktop's Settings (ADR-0038), a Patina app
//! for the settings that need more than the desktop's menu: the skin, the
//! bar, the wallpaper and Night Light. It edits the files those settings live in
//! (the bar's config, the compositor's), and the pieces reading them follow.

mod account;
mod app;
mod bar;
mod battery;
mod color_scheme;
mod compositor_config;
mod defaults;
mod dictation;
mod gaps;
mod icons;
mod keyboard;
mod night_light;
mod power;
mod processes;
mod wallpapers;

use std::process::ExitCode;

use crate::app::Page;

const USAGE: &str =
    "usage: patina-settings [--page appearance|bar|wallpaper|defaults|keyboard|dictation|night-light|power|processes]

Opens the Patina desktop's Settings, at the page given (Appearance without
one). The desktop's menu opens it at the wallpaper page, the bar's battery
at the power and battery page.";

fn main() -> ExitCode {
    let mut page = Page::Appearance;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--page" => match args.next().as_deref().and_then(Page::from_name) {
                Some(p) => page = p,
                None => return usage_error(),
            },
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            "-V" | "--version" => {
                println!("patina-settings {}", env!("CARGO_PKG_VERSION"));
                return ExitCode::SUCCESS;
            }
            _ => return usage_error(),
        }
    }
    let options = eframe::NativeOptions {
        viewport: patina::frame::viewport(
            egui::ViewportBuilder::default()
                .with_title("Settings")
                .with_app_id("com.sectorflabs.Settings")
                .with_icon(window_icon())
                .with_inner_size([860.0, 600.0])
                .with_min_inner_size([560.0, 400.0]),
            true,
        ),
        ..Default::default()
    };
    let run = eframe::run_native(
        "patina-settings",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(&cc.egui_ctx, page)))),
    );
    match run {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("patina-settings: {e}");
            ExitCode::FAILURE
        }
    }
}

/// The window icon, drawn from the icon `make install` puts in the theme
/// (`packaging/settings.svg`); on Wayland, the desktop entry's is shown.
fn window_icon() -> egui::IconData {
    const SIDE: u32 = 128;
    let svg = include_str!("../packaging/settings.svg");
    egui::IconData {
        rgba: patina_system::logo::render_svg(svg, SIDE).unwrap_or_default(),
        width: SIDE,
        height: SIDE,
    }
}

fn usage_error() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_icon_is_drawn_from_the_svg() {
        let icon = window_icon();
        assert_eq!(icon.rgba.len(), 128 * 128 * 4);
        // The gear's orange centre, opaque.
        let at = |x: usize, y: usize| &icon.rgba[(y * 128 + x) * 4..][..4];
        assert_eq!(at(92, 34)[3], 255);
        assert!(at(92, 34)[0] > 200, "{:?}", at(92, 34));
        // A corner, clear.
        assert_eq!(at(1, 1)[3], 0);
    }

    /// Draws the icon as icon.png in PATINA_SETTINGS_PREVIEW:
    /// `cargo test -p patina-settings preview_the_icon -- --ignored`.
    #[test]
    #[ignore = "writes a file"]
    fn preview_the_icon() {
        let dir = std::path::PathBuf::from(std::env::var("PATINA_SETTINGS_PREVIEW").unwrap());
        let icon = window_icon();
        image::save_buffer(
            dir.join("icon.png"),
            &icon.rgba,
            icon.width,
            icon.height,
            image::ColorType::Rgba8,
        )
        .unwrap();
    }
}
