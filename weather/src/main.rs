//! Weather: a Patina weather app, after macOS Weather. The weather now, the
//! days ahead, each day in detail, and graphs of it all, from Open-Meteo.

mod app;
mod charts;
mod config;
mod day;
mod fetch;
mod forecast;
mod icons;
mod overview;
mod palette;
mod units;
mod week;
mod widgets;

/// The window icon, rendered from `packaging/linux/weather.svg` by `make
/// icons`. On Wayland, GNOME shows the installed launcher's icon instead.
const WINDOW_ICON_PNG: &[u8] = include_bytes!("../assets/weather-icon-128.png");

fn window_icon() -> egui::IconData {
    let icon = image::load_from_memory_with_format(WINDOW_ICON_PNG, image::ImageFormat::Png)
        .expect("embedded window icon should decode as PNG")
        .into_rgba8();
    let (width, height) = icon.dimensions();
    egui::IconData { rgba: icon.into_raw(), width, height }
}

fn main() -> eframe::Result {
    let config = config::Config::load();
    let options = eframe::NativeOptions {
        viewport: patina::frame::viewport(
            egui::ViewportBuilder::default()
                .with_title("Weather")
                .with_app_id("com.sectorflabs.Weather")
                .with_icon(window_icon())
                .with_inner_size([1240.0, 860.0])
                .with_min_inner_size([640.0, 480.0]),
            !config.system_title_bar,
        ),
        ..Default::default()
    };
    eframe::run_native("weather", options, Box::new(|cc| Ok(Box::new(app::App::new(cc)))))
}

#[cfg(test)]
mod tests {
    #[test]
    fn window_icon_decodes_at_its_size() {
        let icon = super::window_icon();
        assert_eq!((icon.width, icon.height), (128, 128));
    }
}
