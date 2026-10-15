//! patina-launcher: the Patina desktop's app launcher. It lists the apps
//! installed through `.desktop` files, filters them as you type, and starts
//! the one you pick. Running it while it's open closes it, so one button
//! toggles it.

mod apps;
mod icons;
mod ui;

use std::process::ExitCode;

use patina_shell::calloop::channel::{self, Event};
use patina_shell::piece::{self, Single};
use patina_shell::{Anchor, Host, KeyboardInteractivity, Layer, Options};

const USAGE: &str = "usage: patina-launcher

Lists the installed apps, filters them as you type and starts the one you
pick. Run it again while it's open to close it. It opens at the left end of
the bar, like a menu from the bar's button: the edge named by
PATINA_BAR_EDGE (top or bottom, as patina-bar sets; bottom by default). The
skin is the one named by PATINA_SKIN, or Default.";

/// The launcher's size in points.
const SIZE: (u32, u32) = (420, 500);
/// Its distance from the bar and the screen's left edge, as a menu sits
/// just off the button that opened it.
const GAP: i32 = 4;

enum Message {
    Icon(usize, egui::ColorImage),
    Close,
}

fn main() -> ExitCode {
    if let Some(arg) = std::env::args().nth(1) {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            "-V" | "--version" => {
                println!("patina-launcher {}", env!("CARGO_PKG_VERSION"));
                return ExitCode::SUCCESS;
            }
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("patina-launcher: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let (send, receive) = channel::channel::<Message>();
    let closer = send.clone();
    // Already open: ask that one to close, and stop.
    let Some(_single) = Single::start("patina-launcher", "close", move |word| {
        word != "close" || closer.send(Message::Close).is_ok()
    })?
    else {
        return Ok(());
    };

    let apps = apps::load();
    let wanted: Vec<(usize, String)> = apps
        .iter()
        .enumerate()
        .filter_map(|(i, a)| a.icon.clone().map(|icon| (i, icon)))
        .collect();
    let start_icons = Box::new(move |size: u32| {
        icons::spawn(wanted, size, move |i, image| {
            send.send(Message::Icon(i, image)).is_ok()
        });
    });
    let launcher = ui::Launcher::new(apps, piece::skin("patina-launcher"), start_icons);

    let options = Options {
        namespace: "patina-launcher".into(),
        layer: Layer::Overlay,
        anchor: piece::bar_edge() | Anchor::LEFT,
        size: SIZE,
        // 0 keeps it clear of the bar's reserved strip.
        exclusive_zone: 0,
        margin: (GAP, 0, GAP, GAP),
        keyboard: KeyboardInteractivity::Exclusive,
        per_output: false,
        toplevels: false,
        lock: false,
    };
    let (host, event_loop) = Host::connect(options, launcher)?;
    event_loop
        .handle()
        .insert_source(receive, |event, _, host| match event {
            Event::Msg(Message::Icon(i, image)) => {
                host.shell.pending.push((i, image));
                host.redraw_all();
            }
            Event::Msg(Message::Close) => host.shell.close(),
            Event::Closed => {}
        })
        .map_err(|e| e.to_string())?;
    host.run(event_loop)
}
