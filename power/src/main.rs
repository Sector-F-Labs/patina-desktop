//! patina-power: the power menu at the right end of the Patina desktop's
//! bar (ADR-0048): Lock, Log Out, Restart and Shut Down. The bar's power
//! button runs it; running it again closes it.

mod ui;

use std::process::ExitCode;
use std::sync::mpsc;

use patina_shell::calloop::channel::{self, Event};
use patina_shell::piece::{self, Single};
use patina_shell::{Anchor, Host, KeyboardInteractivity, Layer, Options};
use patina_system::session::{self, Power};

use crate::ui::{Action, PowerMenu};

const USAGE: &str = "usage: patina-power

Opens the power menu at the bar's right end, or closes it if it's open:
Lock, Log Out, Restart and Shut Down. PATINA_SKIN and PATINA_HUD_EDGE, as
patina-hud sets them, choose the skin and which edge it opens from.";

const SIZE: (u32, u32) = (260, 196);
const GAP: i32 = 4;
/// What locks the screen where the compositor can't be asked to.
const LOCKER: &str = "patina-lock";

enum Message {
    Abilities(session::Abilities),
    /// An action failed; the menu says why.
    Failed(String),
    Close,
}

fn main() -> ExitCode {
    match std::env::args().nth(1).as_deref() {
        None => {}
        Some("-h" | "--help") => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Some("-V" | "--version") => {
            println!("patina-power {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Some(_) => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    }
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("patina-power: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let (send, receive) = channel::channel::<Message>();
    let closer = send.clone();
    let Some(_single) = Single::start("patina-power", "close", move |word| {
        word != "close" || closer.send(Message::Close).is_ok()
    })?
    else {
        return Ok(());
    };

    let (actions, chosen) = mpsc::channel::<Action>();
    std::thread::Builder::new()
        .name("session".into())
        .spawn(move || worker(&send, &chosen))
        .map_err(|e| e.to_string())?;

    let menu = PowerMenu::new(piece::skin("patina-power"), actions);
    // At the button that opened it, or on the bar's edge (ADR-0063).
    let (anchor, margin, exclusive_zone) = piece::opening(SIZE, GAP, Anchor::RIGHT);
    let options = Options {
        namespace: "patina-power".into(),
        layer: Layer::Overlay,
        anchor,
        size: SIZE,
        exclusive_zone,
        margin,
        keyboard: KeyboardInteractivity::Exclusive,
        per_output: false,
        toplevels: false,
        click_through: false,
        lock: false,
        window_fallback: false,
    };
    let (host, event_loop) = Host::connect(options, menu)?;
    event_loop
        .handle()
        .insert_source(receive, |event, _, host| {
            match event {
                Event::Msg(Message::Abilities(a)) => host.shell.abilities = Some(a),
                Event::Msg(Message::Failed(e)) => host.shell.failed(e),
                Event::Msg(Message::Close) => host.shell.close(),
                Event::Closed => return,
            }
            host.redraw_all();
        })
        .map_err(|e| e.to_string())?;
    host.run(event_loop)
}

/// Asks what can be done here, then carries out what's chosen, off the UI
/// thread, since the login manager may take a while (or ask for a
/// password).
fn worker(send: &channel::Sender<Message>, chosen: &mpsc::Receiver<Action>) {
    if send.send(Message::Abilities(session::abilities())).is_err() {
        return;
    }
    while let Ok(action) = chosen.recv() {
        let result = match action {
            Action::Lock => session::lock().or_else(|_| lock_here()),
            Action::LogOut => session::log_out(),
            Action::Restart => session::power(Power::Restart),
            Action::ShutDown => session::power(Power::ShutDown),
        };
        let message = match result {
            Ok(()) => Message::Close,
            Err(e) => Message::Failed(e),
        };
        if send.send(message).is_err() {
            return;
        }
    }
}

/// Runs the lock screen itself, where Patina's compositor isn't there to.
fn lock_here() -> Result<(), String> {
    std::process::Command::new(LOCKER)
        .stdin(std::process::Stdio::null())
        .spawn()
        .map(drop)
        .map_err(|e| format!("{LOCKER}: {e}"))
}
