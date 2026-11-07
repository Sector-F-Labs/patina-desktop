//! patina-skin-builder: Skin Builder, a Patina app for making skins (the suite skin format,
//! Patina's docs/skinning.md) with a live preview, and for applying one to the apps whose
//! folders are in `~/.config`.

mod app;
mod doc;
mod targets;

use std::process::ExitCode;

fn main() -> ExitCode {
    match std::env::args().nth(1).as_deref() {
        None => {}
        Some("-h" | "--help") => {
            println!(
                "usage: patina-skin-builder\n\nOpens Skin Builder. Your skins are kept in \
                 ~/.config/patina-skin-builder/skins."
            );
            return ExitCode::SUCCESS;
        }
        Some("-V" | "--version") => {
            println!("patina-skin-builder {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Some(_) => {
            eprintln!("usage: patina-skin-builder");
            return ExitCode::from(2);
        }
    }
    let options = eframe::NativeOptions {
        viewport: patina::frame::viewport(
            egui::ViewportBuilder::default()
                .with_title("Skin Builder")
                .with_app_id("com.sectorflabs.SkinBuilder")
                .with_inner_size([1180.0, 760.0])
                .with_min_inner_size([760.0, 480.0]),
            true,
        ),
        ..Default::default()
    };
    match eframe::run_native(
        "patina-skin-builder",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    ) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("patina-skin-builder: {e}");
            ExitCode::FAILURE
        }
    }
}
