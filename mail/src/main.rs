//! Mail: a Patina mail app for Proton Mail through Proton Mail Bridge.
//! Folders, a message list and a reading pane over Bridge's IMAP, and
//! writing, replying and forwarding over its SMTP. HTML mail is drawn by
//! Blitz, with no scripts and nothing loaded from outside the message.

mod app;
mod compose;
mod config;
mod demo;
mod html;
mod message;
mod sync;

/// The window icon, rendered from `packaging/linux/mail.svg` by `make
/// icons`. On Wayland, GNOME shows the installed launcher's icon instead.
const WINDOW_ICON_PNG: &[u8] = include_bytes!("../assets/mail-icon-128.png");

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
                .with_title("Mail")
                .with_app_id("com.sectorflabs.Mail")
                .with_icon(window_icon())
                .with_inner_size([1200.0, 780.0])
                .with_min_inner_size([760.0, 420.0]),
            !config.system_title_bar,
        ),
        ..Default::default()
    };
    eframe::run_native("mail", options, Box::new(|cc| Ok(Box::new(app::App::new(cc)))))
}

#[cfg(test)]
mod tests {
    #[test]
    fn window_icon_decodes_at_its_size() {
        let icon = super::window_icon();
        assert_eq!((icon.width, icon.height), (128, 128));
    }
}
