//! Notebook: WYSIWYG markdown notes for a folder of `.md` files, built on
//! egui and Patina.

mod app;
mod config;
mod notes;
mod screenshot;

// The markdown model and the WYSIWYG widget live in Patina (its ADR-0015),
// so Reader and Patina's help show notes the same way.
use patina::markdown::{editor, model as markdown};

use std::path::PathBuf;

/// The window icon, rendered from `packaging/linux/notebook.svg` by `make icons`.
const WINDOW_ICON_PNG: &[u8] = include_bytes!("../assets/notebook-icon-128.png");

fn window_icon() -> egui::IconData {
    let icon = image::load_from_memory_with_format(WINDOW_ICON_PNG, image::ImageFormat::Png)
        .expect("embedded window icon should decode as PNG")
        .into_rgba8();
    let (width, height) = icon.dimensions();
    egui::IconData { rgba: icon.into_raw(), width, height }
}

fn main() -> eframe::Result {
    let open = std::env::args_os().nth(1).map(PathBuf::from).map(|p| p.canonicalize().unwrap_or(p));
    let config = config::Config::load();
    let options = eframe::NativeOptions {
        viewport: patina::frame::viewport(
            egui::ViewportBuilder::default()
                .with_title("Notebook")
                .with_app_id("com.sectorflabs.Notebook")
                .with_icon(window_icon())
                .with_inner_size([1100.0, 740.0])
                .with_min_inner_size([560.0, 360.0]),
            !config.system_title_bar,
        ),
        ..Default::default()
    };
    eframe::run_native("notebook", options, Box::new(|cc| Ok(Box::new(app::App::new(cc, open)))))
}

#[cfg(test)]
mod tests {
    #[test]
    fn window_icon_decodes_at_its_size() {
        let icon = super::window_icon();
        assert_eq!((icon.width, icon.height), (128, 128));
        assert_eq!(icon.rgba.len(), 128 * 128 * 4);
    }
}
