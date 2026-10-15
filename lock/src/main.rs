//! patina-lock: the Patina desktop's lock screen (ADR-0028). It locks the
//! session through ext-session-lock, shows the time in the bar's skin, and
//! unlocks once PAM accepts the password. The compositor runs it from
//! `[lock] command`: Ctrl+Super+Q, Super+Escape, and before sleep.

mod pam;
mod ui;

use std::process::ExitCode;
use std::sync::mpsc;

use patina_shell::calloop::channel::{self, Event};
use patina_shell::piece;
use patina_shell::{Anchor, Host, KeyboardInteractivity, Layer, Options};

use crate::ui::LockScreen;

const USAGE: &str = "usage: patina-lock

Locks the session until your password is typed. Needs a compositor with
ext-session-lock, such as patina-compositor. PATINA_SKIN, as patina-bar
sets it, or the bar's config chooses the skin.";

fn main() -> ExitCode {
    match std::env::args().nth(1).as_deref() {
        None => {}
        Some("-h" | "--help") => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Some("-V" | "--version") => {
            println!("patina-lock {}", env!("CARGO_PKG_VERSION"));
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
            eprintln!("patina-lock: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let (user, full_name) = pam::user().ok_or("can't tell who is running this")?;
    let name = full_name.unwrap_or_else(|| user.clone());

    // Passwords are checked on a thread of their own: PAM blocks, for
    // seconds after a wrong one.
    let (check, checks) = mpsc::channel::<String>();
    let (send, results) = channel::channel::<Result<bool, String>>();
    std::thread::Builder::new()
        .name("pam".into())
        .spawn(move || {
            while let Ok(mut password) = checks.recv() {
                let result = pam::authenticate(&user, &password);
                ui::wipe(&mut password);
                if send.send(result).is_err() {
                    return;
                }
            }
        })
        .map_err(|e| e.to_string())?;

    let screen = LockScreen::new(piece::skin("patina-lock"), name, check);
    let options = Options {
        namespace: "patina-lock".into(),
        layer: Layer::Overlay,
        anchor: Anchor::empty(),
        size: (0, 0),
        exclusive_zone: 0,
        margin: (0, 0, 0, 0),
        keyboard: KeyboardInteractivity::Exclusive,
        per_output: false,
        toplevels: false,
        click_through: false,
        lock: true,
    };
    let (host, event_loop) = Host::connect(options, screen)?;
    event_loop
        .handle()
        .insert_source(results, |event, _, host| {
            if let Event::Msg(result) = event {
                host.shell.checked(result);
                host.redraw_all();
            }
        })
        .map_err(|e| e.to_string())?;
    host.run(event_loop)
}
