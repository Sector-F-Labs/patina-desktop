//! Draw: a simple bitmap paint program, the Unix MS Paint. Built on egui and Patina.

mod app;
mod bitmap;
mod config;
mod document;
mod file;

fn main() -> eframe::Result {
    let config = config::Config::load();
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Draw")
        .with_app_id("com.sectorflabs.Draw")
        .with_inner_size([1100.0, 740.0])
        .with_min_inner_size([560.0, 400.0]);
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../assets/draw-icon-128.png")) {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport: patina::frame::viewport(viewport, !config.system_title_bar),
        ..Default::default()
    };
    eframe::run_native("draw", options, Box::new(|cc| Ok(Box::new(app::App::new(cc, config)))))
}
