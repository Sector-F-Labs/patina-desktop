//! patina-volume: the volume menu of the Patina desktop's bar, which also
//! flashes up when the volume or brightness keys are used (ADR-0021,
//! ADR-0033).
//!
//! `patina-volume` opens the menu, or closes it if it's open; the bar's
//! speaker runs it. `patina-volume --flash` shows the volume briefly, or
//! keeps an open menu up, and `--brightness` the brightness; the compositor
//! runs them after a key. Keys pressed again keep the one flash up.

mod ui;

use std::process::ExitCode;
use std::sync::mpsc;
use std::time::Duration;

use patina_shell::calloop::channel::{self, Event};
use patina_shell::piece::{self, Single};
use patina_shell::{Anchor, Host, KeyboardInteractivity, Layer, Options};

use patina_system::mpris::{self, Command, Player};

use crate::ui::{Change, Mode, Showing, VolumeMenu, height};

const USAGE: &str = "usage: patina-volume [--flash | --brightness]

Opens the volume menu at the bar's right end, or closes it if it's open.
With --flash, shows the volume briefly without taking the keyboard, as
after a volume key, or keeps an open one up; with --brightness, the
screen's brightness, as after a brightness key. PATINA_SKIN and
PATINA_HUD_EDGE, as patina-hud sets them, choose the skin and which edge it
opens from.";

/// Its width; the height is the slider's, and a card for each player in the menu (ADR-0117).
const WIDTH: u32 = 320;
/// Its distance from the bar and the screen's right edge.
const GAP: i32 = 4;
/// How often the levels are read while it's shown, for changes made
/// elsewhere; a key reads them at once.
const POLL: Duration = Duration::from_millis(400);

enum Message {
    Volume(patina_system::audio::Volume),
    Brightness(Option<f32>),
    /// The players with something to control, playing first.
    Players(Vec<Player>),
    Toggle,
    Flash(Showing),
}

fn main() -> ExitCode {
    let (mode, showing) = match std::env::args().nth(1).as_deref() {
        None => (Mode::Menu, Showing::Volume),
        Some("--flash") => (Mode::Flash, Showing::Volume),
        Some("--brightness") => (Mode::Flash, Showing::Brightness),
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
    match run(mode, showing) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("patina-volume: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(mode: Mode, showing: Showing) -> Result<(), String> {
    let (send, receive) = channel::channel::<Message>();
    let commands = send.clone();
    let word = match (mode, showing) {
        (Mode::Menu, _) => "toggle",
        (Mode::Flash, Showing::Volume) => "flash",
        (Mode::Flash, Showing::Brightness) => "brightness",
    };
    // Already shown: tell that one, and stop.
    let Some(_single) = Single::start("patina-volume", word, move |word| {
        let message = match word.as_str() {
            "toggle" => Message::Toggle,
            "flash" => Message::Flash(Showing::Volume),
            "brightness" => Message::Flash(Showing::Brightness),
            _ => return true,
        };
        commands.send(message).is_ok()
    })?
    else {
        return Ok(());
    };

    // The levels, read now, while shown, and at once when poked (after a
    // key, so the number keeps up with the presses).
    let (poke, poked) = mpsc::channel::<()>();
    std::thread::Builder::new()
        .name("poll".into())
        .spawn(move || {
            let mut bus = None;
            loop {
                if bus.is_none() {
                    bus = mpris::connect().ok();
                }
                if let Some(b) = &bus {
                    let all = mpris::players(b).map(mpris::active).unwrap_or_default();
                    if send.send(Message::Players(all)).is_err() {
                        return;
                    }
                }
                if let Some(volume) = patina_system::audio::get()
                    && send.send(Message::Volume(volume)).is_err()
                {
                    return;
                }
                if send
                    .send(Message::Brightness(patina_system::backlight::level()))
                    .is_err()
                {
                    return;
                }
                if let Err(mpsc::RecvTimeoutError::Disconnected) = poked.recv_timeout(POLL) {
                    return;
                }
            }
        })
        .map_err(|e| e.to_string())?;
    // Changes, applied in order.
    let (changes, applied) = mpsc::channel::<Change>();
    std::thread::Builder::new()
        .name("apply".into())
        .spawn(move || {
            while let Ok(mut change) = applied.recv() {
                // A dragged slider sends many levels; only the last matters.
                while let Ok(next) = applied.try_recv() {
                    match (change, next) {
                        (Change::Level(_), Change::Level(_))
                        | (Change::Brightness(_), Change::Brightness(_)) => change = next,
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

    // What the cards ask of the players, in order, off the UI thread: they may take a moment.
    let (player_commands, player_received) = mpsc::channel::<(String, Command)>();
    std::thread::Builder::new()
        .name("players".into())
        .spawn(move || {
            while let Ok((name, command)) = player_received.recv() {
                let sent = mpris::connect().and_then(|bus| mpris::send(&bus, &name, command));
                if let Err(e) = sent {
                    eprintln!("patina-volume: {name}: {e}");
                }
            }
        })
        .map_err(|e| e.to_string())?;

    let mut menu = VolumeMenu::new(
        mode,
        showing,
        piece::skin("patina-volume"),
        changes,
        player_commands,
    );
    // Read once now, so the menu opens as tall as it'll be, not growing a moment later.
    if mode == Mode::Menu {
        menu.players = mpris::connect()
            .and_then(|bus| mpris::players(&bus))
            .map(mpris::active)
            .unwrap_or_default();
    }
    let size = (WIDTH, height(menu.players.len()));
    // At the button that opened it, or on the bar's edge (ADR-0063).
    let (anchor, margin, exclusive_zone) = piece::opening(size, GAP, Anchor::RIGHT);
    let options = Options {
        namespace: "patina-volume".into(),
        layer: Layer::Overlay,
        anchor,
        size: (
            WIDTH,
            if mode == Mode::Menu {
                size.1
            } else {
                height(0)
            },
        ),
        exclusive_zone,
        margin,
        keyboard: keyboard_for(mode),
        per_output: false,
        toplevels: false,
        click_through: false,
        lock: false,
        window_fallback: false,
    };
    let (mut host, event_loop) = Host::connect(options, menu)?;
    host.set_shadow(patina_shell::hud::shadow());
    event_loop
        .handle()
        .insert_source(receive, move |event, _, host| {
            match event {
                Event::Msg(Message::Volume(volume)) => {
                    if host.shell.volume != Some(volume) {
                        host.shell.volume = Some(volume);
                        host.redraw_all();
                    }
                    return;
                }
                Event::Msg(Message::Brightness(level)) => {
                    if host.shell.brightness != level {
                        host.shell.brightness = level;
                        host.redraw_all();
                    }
                    return;
                }
                Event::Msg(Message::Players(players)) => {
                    if host.shell.players != players {
                        host.shell.players = players;
                        // Only the menu lists them; a flash is the slider alone.
                        if host.shell.mode() == Mode::Menu {
                            host.set_size((WIDTH, height(host.shell.players.len())));
                        }
                        host.redraw_all();
                    }
                    return;
                }
                Event::Msg(Message::Flash(showing)) => {
                    host.shell.flash(showing);
                    let _ = poke.send(());
                }
                Event::Msg(Message::Toggle) => match host.shell.mode() {
                    Mode::Menu => host.shell.close(),
                    Mode::Flash => {
                        host.shell.open();
                        host.set_size((WIDTH, height(host.shell.players.len())));
                        host.set_keyboard(keyboard_for(Mode::Menu));
                    }
                },
                Event::Closed => return,
            }
            host.redraw_all();
        })
        .map_err(|e| e.to_string())?;
    host.run(event_loop)
}

fn apply(change: Change) {
    match change {
        Change::Level(level) => patina_system::audio::set_level(level),
        Change::Muted(muted) => patina_system::audio::set_muted(muted),
        Change::Brightness(level) => {
            if let Err(e) = patina_system::backlight::set(level) {
                eprintln!("patina-volume: brightness: {e}");
            }
        }
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
