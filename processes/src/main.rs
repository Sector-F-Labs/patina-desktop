//! patina-processes: the Patina desktop's process manager (ADR-0088), a Patina
//! app on Ctrl+Shift+Escape, a key no config can take: what's running,
//! grouped by the apps whose windows are open, what each costs the
//! machine, and ending, killing or pausing it; and graphs of the CPU,
//! memory, disks and network.

mod app;
mod details;
mod gpu;
mod graph;
mod groups;
mod icons;
mod overview;
mod performance;
mod processes;
mod ranking;
mod sample;
mod window;

use std::process::ExitCode;

use crate::app::View;

const USAGE: &str = "usage: patina-processes [--processes | --performance]

Opens the process manager at its overview, or at its list of processes or
its graphs. The desktop opens it on Ctrl+Shift+Escape.";

fn main() -> ExitCode {
    let mut view = View::Overview;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--processes" => view = View::Processes,
            "--performance" => view = View::Performance,
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            "-V" | "--version" => {
                println!("patina-processes {}", env!("CARGO_PKG_VERSION"));
                return ExitCode::SUCCESS;
            }
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let options = eframe::NativeOptions {
        viewport: patina::frame::viewport(
            egui::ViewportBuilder::default()
                .with_title("Process Manager")
                .with_app_id("com.sectorflabs.ProcessManager")
                .with_icon(window_icon())
                .with_inner_size([1240.0, 840.0])
                .with_min_inner_size([760.0, 520.0]),
            true,
        ),
        ..Default::default()
    };
    let run = eframe::run_native(
        "patina-processes",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(&cc.egui_ctx, view)))),
    );
    match run {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("patina-processes: {e}");
            ExitCode::FAILURE
        }
    }
}

/// The window icon, drawn from the icon `make install` puts in the theme
/// (`packaging/processes.svg`); on Wayland, the desktop entry's is shown.
fn window_icon() -> egui::IconData {
    const SIDE: u32 = 128;
    let svg = include_str!("../packaging/processes.svg");
    egui::IconData {
        rgba: patina_system::logo::render_svg(svg, SIDE).unwrap_or_default(),
        width: SIDE,
        height: SIDE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_icon_is_drawn_from_the_svg() {
        let icon = window_icon();
        assert_eq!(icon.rgba.len(), 128 * 128 * 4);
        // The bezel, opaque; a corner, clear.
        let at = |x: usize, y: usize| &icon.rgba[(y * 128 + x) * 4..][..4];
        assert_eq!(at(64, 20)[3], 255);
        assert_eq!(at(1, 1)[3], 0);
    }
}
