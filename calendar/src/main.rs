//! Calendar: a Patina calendar app for a CalDAV server, made for `plan` on
//! urd. Day, week and month views, calendars in colour that can be shown
//! and hidden, and events that can be added, changed and deleted.

mod app;
mod caldav;
mod config;
mod edit;
mod ics;
mod model;
mod sync;
mod views;

/// The window icon, rendered from `packaging/linux/calendar.svg` by `make
/// icons`. On Wayland, GNOME shows the installed launcher's icon instead.
const WINDOW_ICON_PNG: &[u8] = include_bytes!("../assets/calendar-icon-128.png");

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
                .with_title("Calendar")
                .with_app_id("com.sectorflabs.Calendar")
                .with_icon(window_icon())
                .with_inner_size([1200.0, 800.0])
                .with_min_inner_size([720.0, 480.0]),
            !config.system_title_bar,
        ),
        ..Default::default()
    };
    eframe::run_native("calendar", options, Box::new(|cc| Ok(Box::new(app::App::new(cc)))))
}

#[cfg(test)]
mod tests {
    #[test]
    fn window_icon_decodes_at_its_size() {
        let icon = super::window_icon();
        assert_eq!((icon.width, icon.height), (128, 128));
    }
}
