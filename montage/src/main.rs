//! Montage: a simple home video editor, in the spirit of iMovie and Windows
//! Movie Maker rather than Premiere. Clips go in a library, are dragged onto
//! a timeline and trimmed, with a music track and a voiceover under them,
//! and the movie is exported as an MP4. Built on egui and Patina; ffmpeg
//! does the decoding and encoding.

mod app;
mod config;
mod library;
mod media;
mod preview;
mod project;
mod record;
mod render;
#[cfg(test)]
mod testing;
mod timeline;

/// The window icon, rendered from `packaging/linux/montage.svg` by `make
/// icons`. On Wayland, the desktop shows the installed launcher's icon instead.
const WINDOW_ICON_PNG: &[u8] = include_bytes!("../assets/montage-icon-128.png");

fn window_icon() -> egui::IconData {
    let icon = image::load_from_memory_with_format(WINDOW_ICON_PNG, image::ImageFormat::Png)
        .expect("embedded window icon should decode as PNG")
        .into_rgba8();
    let (width, height) = icon.dimensions();
    egui::IconData { rgba: icon.into_raw(), width, height }
}

const USAGE: &str = "usage: montage [PROJECT.montage]

Opens PROJECT, or else the project open when Montage last closed.";

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("{USAGE}");
        return std::process::ExitCode::SUCCESS;
    }
    if args.len() > 1 || args.first().is_some_and(|a| a.starts_with('-')) {
        eprintln!("{USAGE}");
        return std::process::ExitCode::from(2);
    }
    let open = args.into_iter().next().map(std::path::PathBuf::from);
    let config = config::Config::load();
    let options = eframe::NativeOptions {
        viewport: patina::frame::viewport(
            egui::ViewportBuilder::default()
                .with_title("Montage")
                .with_app_id("com.sectorflabs.Montage")
                .with_icon(window_icon())
                .with_inner_size([1280.0, 820.0])
                .with_min_inner_size([860.0, 600.0])
                .with_drag_and_drop(true),
            !config.system_title_bar,
        ),
        ..Default::default()
    };
    match eframe::run_native("montage", options, Box::new(|cc| Ok(Box::new(app::App::new(cc, open))))) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("montage: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn window_icon_decodes_at_its_size() {
        let icon = super::window_icon();
        assert_eq!((icon.width, icon.height), (128, 128));
    }
}
