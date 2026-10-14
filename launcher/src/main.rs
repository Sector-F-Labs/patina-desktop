//! patina-launcher: the Patina desktop's app launcher. It lists the apps
//! installed through `.desktop` files, filters them as you type, and starts
//! the one you pick. Running it while it's open closes it, so one button
//! toggles it.

mod apps;
mod icons;
mod ui;

use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::ExitCode;

use patina::skin::{self, Skin};
use patina_shell::calloop::channel::{self, Event};
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
    let socket = socket_path();
    // Already open: ask that one to close, and stop.
    if let Ok(mut open) = UnixStream::connect(&socket) {
        let _ = open.write_all(b"close");
        return Ok(());
    }
    let _ = std::fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket).map_err(|e| format!("{}: {e}", socket.display()))?;

    let (send, receive) = channel::channel::<Message>();
    let closer = send.clone();
    std::thread::Builder::new()
        .name("toggle".into())
        .spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let mut word = String::new();
                let _ = stream.read_to_string(&mut word);
                if word == "close" && closer.send(Message::Close).is_err() {
                    return;
                }
            }
        })
        .map_err(|e| e.to_string())?;

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
    let launcher = ui::Launcher::new(apps, chosen_skin(), start_icons);

    let options = Options {
        namespace: "patina-launcher".into(),
        layer: Layer::Overlay,
        anchor: bar_edge() | Anchor::LEFT,
        size: SIZE,
        // 0 keeps it clear of the bar's reserved strip.
        exclusive_zone: 0,
        margin: (GAP, 0, GAP, GAP),
        keyboard: KeyboardInteractivity::Exclusive,
        per_output: false,
        toplevels: false,
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
    let result = host.run(event_loop);
    let _ = std::fs::remove_file(&socket);
    result
}

/// The bar's edge, from `PATINA_BAR_EDGE`, which the launcher opens from.
fn bar_edge() -> Anchor {
    if patina_shell::bar::at_top() {
        Anchor::TOP
    } else {
        Anchor::BOTTOM
    }
}

/// One socket per Wayland display, in the runtime directory.
fn socket_path() -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map_or_else(std::env::temp_dir, PathBuf::from);
    let display = std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-0".into());
    dir.join(format!("patina-launcher-{display}.sock"))
}

/// The skin named by `PATINA_SKIN`, from Patina's built-ins and the
/// launcher's own skins folder, or Default.
fn chosen_skin() -> Skin {
    let Some(name) = patina_shell::bar::skin() else {
        return Skin::default_skin();
    };
    let dirs: Vec<_> = skin::app_dir("patina-launcher").into_iter().collect();
    let (skins, _) = skin::load_all(skin::BUILT_IN, &dirs);
    skins
        .into_iter()
        .find(|s| s.name == name)
        .unwrap_or_else(Skin::default_skin)
}
