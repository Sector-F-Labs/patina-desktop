//! Ledger: bookkeeping in tabbed spreadsheets stored as SQLite databases.
//! Built on egui and Patina.

mod app;
mod config;
mod demo;
mod formula;
mod grid;
mod sheet;
mod store;

fn main() -> eframe::Result {
    let config = config::Config::load();
    let open = std::env::args_os().nth(1).map(std::path::PathBuf::from);
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/ledger-icon-128.png")).ok();
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Ledger")
        .with_app_id("com.sectorflabs.Ledger")
        .with_inner_size([1100.0, 740.0])
        .with_min_inner_size([560.0, 360.0]);
    if let Some(icon) = icon {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport: patina::frame::viewport(viewport, !config.system_title_bar),
        ..Default::default()
    };
    eframe::run_native("ledger", options, Box::new(move |cc| Ok(Box::new(app::App::new(cc, config, open)))))
}
