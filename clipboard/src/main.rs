//! patina-clipboard: the clipboard menu of the Patina desktop's bar (ADR-0114): the copies the
//! compositor keeps, to pick, delete or clear. The bar's clipboard icon runs it; running it
//! again closes it.

mod ui;

use std::process::ExitCode;
use std::sync::mpsc;

use patina_shell::calloop::channel::{self, Event};
use patina_shell::piece::{self, Single};
use patina_shell::{Anchor, Host, KeyboardInteractivity, Layer, Options};
use patina_system::clipboard::{self, Status};

use crate::ui::{ClipboardMenu, Command};

const USAGE: &str = "usage: patina-clipboard

Opens the clipboard menu at the bar's right end, or closes it if it's open.
PATINA_SKIN and PATINA_HUD_EDGE, as patina-hud sets them, choose the skin
and which edge it opens from.";

const SIZE: (u32, u32) = (380, 480);
const GAP: i32 = 4;

enum Message {
    /// The history, or `None` while the compositor doesn't serve it.
    Status(Option<Status>),
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
            println!("patina-clipboard {}", env!("CARGO_PKG_VERSION"));
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
            eprintln!("patina-clipboard: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let (send, receive) = channel::channel::<Message>();
    let closer = send.clone();
    let Some(_single) = Single::start("patina-clipboard", "close", move |word| {
        word != "close" || closer.send(Message::Close).is_ok()
    })?
    else {
        return Ok(());
    };

    // The history now and as it changes, off the UI thread.
    std::thread::Builder::new()
        .name("history".into())
        .spawn(move || clipboard::watch(|status| send.send(Message::Status(status)).is_ok()))
        .map_err(|e| e.to_string())?;
    // What the menu asks of the compositor, in order.
    let (commands, received) = mpsc::channel::<Command>();
    let worker = std::thread::Builder::new()
        .name("commands".into())
        .spawn(move || {
            while let Ok(command) = received.recv() {
                if let Err(e) = apply(command) {
                    eprintln!("patina-clipboard: {e}");
                }
            }
        })
        .map_err(|e| e.to_string())?;

    let menu = ClipboardMenu::new(piece::skin("patina-clipboard"), commands);
    // At the button that opened it, or on the bar's edge (ADR-0063).
    let (anchor, margin, exclusive_zone) = piece::opening(SIZE, GAP, Anchor::RIGHT);
    let options = Options {
        namespace: "patina-clipboard".into(),
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
                Event::Msg(Message::Status(status)) => {
                    if host.shell.status == status {
                        return;
                    }
                    host.shell.status = status;
                }
                Event::Msg(Message::Close) => host.shell.close(),
                Event::Closed => return,
            }
            host.redraw_all();
        })
        .map_err(|e| e.to_string())?;
    let result = host.run(event_loop);
    // The menu, and with it the commands' sender, is gone: let the last ones, a pick that
    // closed it, reach the compositor before exiting.
    let _ = worker.join();
    result
}

fn apply(command: Command) -> Result<(), String> {
    match command {
        Command::Pick(id) => clipboard::pick(id),
        Command::Delete(id) => clipboard::delete(id),
        Command::Clear => clipboard::clear(),
        Command::Paused(paused) => clipboard::set_paused(paused),
    }
}
