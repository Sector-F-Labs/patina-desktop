//! Podcasts: a Patina podcast player. Subscribe to any podcast's RSS feed,
//! play episodes while they download, and pick up where you left off.

mod app;
mod config;
mod download;
mod feed;
mod fetch;
mod player;

/// The window icon, rendered from `packaging/linux/podcasts.svg` by `make
/// icons`. On Wayland, GNOME shows the installed launcher's icon instead.
const WINDOW_ICON_PNG: &[u8] = include_bytes!("../assets/podcasts-icon-128.png");

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
                .with_title("Podcasts")
                .with_app_id("com.sectorflabs.Podcasts")
                .with_icon(window_icon())
                .with_inner_size([1100.0, 760.0])
                .with_min_inner_size([640.0, 420.0]),
            !config.system_title_bar,
        ),
        ..Default::default()
    };
    eframe::run_native("podcasts", options, Box::new(|cc| Ok(Box::new(app::App::new(cc)))))
}

#[cfg(test)]
mod tests {
    #[test]
    fn window_icon_decodes_at_its_size() {
        let icon = super::window_icon();
        assert_eq!((icon.width, icon.height), (128, 128));
    }
}
