//! Home: a Patina interface for Home Assistant. Built on egui.

mod app;
mod chart;
mod config;
mod dashboard;
mod ha;
mod history;
mod light;
mod palette;
mod widgets;

fn main() -> eframe::Result {
    let custom_frame = !config::Config::load().system_title_bar;
    let icon = image::load_from_memory(include_bytes!("../assets/home-icon-128.png")).ok().map(|i| egui::IconData {
        width: i.width(),
        height: i.height(),
        rgba: i.into_rgba8().into_raw(),
    });
    let mut builder = egui::ViewportBuilder::default()
        .with_title("Home")
        .with_app_id("com.sectorflabs.Home")
        .with_inner_size([1000.0, 700.0])
        .with_min_inner_size([560.0, 360.0]);
    if let Some(icon) = icon {
        builder = builder.with_icon(icon);
    }
    let options =
        eframe::NativeOptions { viewport: patina::frame::viewport(builder, custom_frame), ..Default::default() };
    eframe::run_native("home", options, Box::new(|cc| Ok(Box::new(app::App::new(cc)))))
}
