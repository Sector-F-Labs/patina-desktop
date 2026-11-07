//! patina-dbug: Dbug, a live viewer for the D-Bus messages on the session bus, a
//! Patina app. It watches as a monitor (`BecomeMonitor`), so it can read every
//! message and send none.

mod app;
mod compose;
mod explain;
mod monitor;
mod send;
mod value;

use std::process::ExitCode;

const USAGE: &str = "usage: patina-dbug [--match RULE]...

Opens Dbug on the session bus. Each --match is a D-Bus match rule, such as
type='signal',interface='org.freedesktop.DBus'; with none, everything is shown.";

fn main() -> ExitCode {
    let mut rules = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--match" => match args.next() {
                Some(rule) => rules.push(rule),
                None => {
                    eprintln!("{USAGE}");
                    return ExitCode::from(2);
                }
            },
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            "-V" | "--version" => {
                println!("patina-dbug {}", env!("CARGO_PKG_VERSION"));
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
                .with_title("Dbug")
                .with_app_id("com.sectorflabs.Dbug")
                .with_inner_size([1200.0, 760.0])
                .with_min_inner_size([640.0, 400.0]),
            true,
        ),
        ..Default::default()
    };
    match eframe::run_native(
        "patina-dbug",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, rules)))),
    ) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("patina-dbug: {e}");
            ExitCode::FAILURE
        }
    }
}
