//! Text Editor: a Patina text editor, in the manner of GNOME's: tabs,
//! find and replace, syntax highlighting, and unsaved work kept across
//! restarts.

mod app;
mod config;
mod document;
mod files;
mod find;
mod indent;
mod instance;
mod session;
mod view;
mod watch;

use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage: text-editor [FILE...]

Opens each FILE in a tab, beside the tabs open last time; a FILE that isn't
there yet is created when saved. If Text Editor is already running, the
files open in its window instead.";

fn main() -> ExitCode {
    let mut files = Vec::new();
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            "-V" | "--version" => {
                println!("text-editor {}", env!("CARGO_PKG_VERSION"));
                return ExitCode::SUCCESS;
            }
            "--" => {}
            a if a.starts_with('-') && a != "-" => {
                eprintln!("text-editor: unknown option {a}\n\n{USAGE}");
                return ExitCode::from(2);
            }
            _ => files.push(as_path(&arg)),
        }
    }
    let screenshot = std::env::var_os("TEXT_EDITOR_SCREENSHOT").is_some();
    if !screenshot && instance::forward(&files) {
        return ExitCode::SUCCESS;
    }
    let options = eframe::NativeOptions {
        viewport: patina::frame::viewport(
            egui::ViewportBuilder::default()
                .with_title("Text Editor")
                .with_app_id("com.sectorflabs.TextEditor")
                .with_inner_size([900.0, 680.0])
                .with_min_inner_size([420.0, 300.0])
                .with_drag_and_drop(true)
                .with_icon(icon()),
            true,
        ),
        ..Default::default()
    };
    match eframe::run_native("text-editor", options, Box::new(move |cc| Ok(Box::new(app::App::new(cc, files))))) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("text-editor: {e}");
            ExitCode::FAILURE
        }
    }
}

/// The window's icon (X11; Wayland takes it from the installed launcher).
fn icon() -> egui::IconData {
    let png = include_bytes!("../assets/text-editor-128.png");
    let image = image::load_from_memory(png).expect("the icon is a PNG").into_rgba8();
    let (width, height) = image.dimensions();
    egui::IconData { rgba: image.into_raw(), width, height }
}

/// An argument as an absolute path; `file://` URIs as their path.
fn as_path(arg: &str) -> PathBuf {
    let path = arg.strip_prefix("file://").map_or_else(|| PathBuf::from(arg), PathBuf::from);
    std::path::absolute(&path).unwrap_or(path)
}
