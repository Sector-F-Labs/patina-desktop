//! patina-volume: the volume menu of the Patina desktop's bar, which also
//! flashes up when the volume keys are used (ADR-0021).
//!
//! `patina-volume` opens the menu, or closes it if it's open; the bar's
//! speaker runs it. `patina-volume --flash` shows it briefly, or keeps an
//! open one up; the compositor runs it after a volume key.

mod ui;

use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::mpsc;
use std::time::Duration;

use patina::skin::{self, Skin};
use patina_shell::calloop::channel::{self, Event};
use patina_shell::{Anchor, Host, KeyboardInteractivity, Layer, Options};

use crate::ui::{Change, Mode, VolumeMenu};

const USAGE: &str = "usage: patina-volume [--flash]

Opens the volume menu at the bar's right end, or closes it if it's open.
With --flash, shows it briefly without taking the keyboard, as after a
volume key, or keeps an open one up. PATINA_SKIN and PATINA_BAR_EDGE, as
patina-bar sets them, choose the skin and which edge it opens from.";

const SIZE: (u32, u32) = (320, 56);
/// Its distance from the bar and the screen's right edge.
const GAP: i32 = 4;
/// How often the level is read while it's shown, for changes made
/// elsewhere.
const POLL: Duration = Duration::from_millis(400);

enum Message {
    Volume(patina_shell::audio::Volume),
    Toggle,
    Flash,
}

fn main() -> ExitCode {
    let mode = match std::env::args().nth(1).as_deref() {
        None => Mode::Menu,
        Some("--flash") => Mode::Flash,
        Some("-h" | "--help") => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Some("-V" | "--version") => {
            println!("patina-volume {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Some(_) => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match run(mode) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("patina-volume: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(mode: Mode) -> Result<(), String> {
    let socket = socket_path();
    // Already shown: tell that one, and stop.
    if let Ok(mut open) = UnixStream::connect(&socket) {
        let word: &[u8] = match mode {
            Mode::Menu => b"toggle",
            Mode::Flash => b"flash",
        };
        let _ = open.write_all(word);
        return Ok(());
    }
    let _ = std::fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket).map_err(|e| format!("{}: {e}", socket.display()))?;

    let (send, receive) = channel::channel::<Message>();
    let commands = send.clone();
    std::thread::Builder::new()
        .name("commands".into())
        .spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let mut word = String::new();
                let _ = stream.read_to_string(&mut word);
                let message = match word.as_str() {
                    "toggle" => Message::Toggle,
                    "flash" => Message::Flash,
                    _ => continue,
                };
                if commands.send(message).is_err() {
                    return;
                }
            }
        })
        .map_err(|e| e.to_string())?;

    // The level, read now and while shown; and changes, applied in order.
    std::thread::Builder::new()
        .name("poll".into())
        .spawn(move || {
            loop {
                if let Some(volume) = patina_shell::audio::get()
                    && send.send(Message::Volume(volume)).is_err()
                {
                    return;
                }
                std::thread::sleep(POLL);
            }
        })
        .map_err(|e| e.to_string())?;
    let (changes, applied) = mpsc::channel::<Change>();
    std::thread::Builder::new()
        .name("audio".into())
        .spawn(move || {
            while let Ok(mut change) = applied.recv() {
                // A dragged slider sends many levels; only the last matters.
                while let Ok(next) = applied.try_recv() {
                    match (change, next) {
                        (Change::Level(_), Change::Level(_)) => change = next,
                        _ => {
                            apply(change);
                            change = next;
                        }
                    }
                }
                apply(change);
            }
        })
        .map_err(|e| e.to_string())?;

    let menu = VolumeMenu::new(mode, chosen_skin(), changes);
    let options = Options {
        namespace: "patina-volume".into(),
        layer: Layer::Overlay,
        anchor: bar_edge() | Anchor::RIGHT,
        size: SIZE,
        exclusive_zone: 0,
        margin: (GAP, GAP, GAP, GAP),
        keyboard: keyboard_for(mode),
        per_output: false,
        toplevels: false,
    };
    let (host, event_loop) = Host::connect(options, menu)?;
    event_loop
        .handle()
        .insert_source(receive, |event, _, host| {
            match event {
                Event::Msg(Message::Volume(volume)) => {
                    if host.shell.volume != Some(volume) {
                        host.shell.volume = Some(volume);
                        host.redraw_all();
                    }
                    return;
                }
                Event::Msg(Message::Flash) => host.shell.flash(),
                Event::Msg(Message::Toggle) => match host.shell.mode() {
                    Mode::Menu => host.shell.close(),
                    Mode::Flash => {
                        host.shell.open();
                        host.set_keyboard(keyboard_for(Mode::Menu));
                    }
                },
                Event::Closed => return,
            }
            host.redraw_all();
        })
        .map_err(|e| e.to_string())?;
    let result = host.run(event_loop);
    let _ = std::fs::remove_file(&socket);
    result
}

fn apply(change: Change) {
    match change {
        Change::Level(level) => patina_shell::audio::set_level(level),
        Change::Muted(muted) => patina_shell::audio::set_muted(muted),
    }
}

/// A menu takes the keyboard (Escape, and closing when it's lost); a flash
/// leaves it with the window you're typing in.
fn keyboard_for(mode: Mode) -> KeyboardInteractivity {
    match mode {
        Mode::Menu => KeyboardInteractivity::Exclusive,
        Mode::Flash => KeyboardInteractivity::None,
    }
}

/// The bar's edge, from `PATINA_BAR_EDGE`.
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
    dir.join(format!("patina-volume-{display}.sock"))
}

/// The skin named by `PATINA_SKIN`, from Patina's built-ins, or Default.
fn chosen_skin() -> Skin {
    let Some(name) = patina_shell::bar::skin() else {
        return Skin::default_skin();
    };
    let dirs: Vec<_> = skin::app_dir("patina-volume").into_iter().collect();
    let (skins, _) = skin::load_all(skin::BUILT_IN, &dirs);
    skins
        .into_iter()
        .find(|s| s.name == name)
        .unwrap_or_else(Skin::default_skin)
}
