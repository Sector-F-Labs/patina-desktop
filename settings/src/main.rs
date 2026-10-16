//! patina-settings: the Patina desktop's Settings (ADR-0038), a Patina app
//! for the settings that need more than the desktop's menu: the skin, the
//! wallpaper and Night Light. It edits the files those settings live in
//! (the bar's config, the compositor's), and the pieces reading them follow.

mod app;
mod compositor_config;
mod night_light;
mod power;
mod wallpapers;

use std::process::ExitCode;

use crate::app::Page;

const USAGE: &str = "usage: patina-settings [--page appearance|wallpaper|night-light|power]

Opens the Patina desktop's Settings, at the page given (Appearance without
one). The desktop's menu opens it at the wallpaper page.";

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

fn usage_error() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(2)
}
