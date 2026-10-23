//! patina-clock: the menu of the Patina desktop's clock (ADR-0082): a month
//! and the chosen day's events, which come from the Calendar app (`calendar
//! --agenda`); a click opens Calendar there. The bar's clock runs it;
//! running it again closes it.

mod agenda;
mod ui;

use std::process::ExitCode;
use std::sync::mpsc;

use patina_shell::calloop::channel::{self, Event};
use patina_shell::piece::{self, Single};
use patina_shell::{Anchor, Host, KeyboardInteractivity, Layer, Options};

use crate::agenda::Item;
use crate::ui::{ClockMenu, Request};

const USAGE: &str = "usage: patina-clock

Opens the clock's menu at the bar's right end, or closes it if it's open: a
month, and the chosen day's events from Calendar (calendar --agenda), which
a click opens Calendar at. PATINA_SKIN and PATINA_BAR_EDGE, as patina-bar
sets them, choose the skin and which edge it opens from.";

const SIZE: (u32, u32) = (320, 500);
const GAP: i32 = 4;

enum Message {
    Agenda(chrono::NaiveDate, Result<Vec<Item>, String>),
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
            println!("patina-clock {}", env!("CARGO_PKG_VERSION"));
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
            eprintln!("patina-clock: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let (send, receive) = channel::channel::<Message>();
    let closer = send.clone();
    let Some(_single) = Single::start("patina-clock", "close", move |word| {
        word != "close" || closer.send(Message::Close).is_ok()
    })?
    else {
        return Ok(());
    };

    // The menu's requests, on a thread: Calendar is a process to wait on.
    let (requests, received) = mpsc::channel::<Request>();
    std::thread::Builder::new()
        .name("calendar".into())
        .spawn(move || {
            while let Ok(request) = received.recv() {
                match request {
                    Request::Agenda(from, days) => {
                        let items = agenda::read(from, days);
                        if send.send(Message::Agenda(from, items)).is_err() {
                            return;
                        }
                    }
                    Request::Run(command) => start(&command),
                }
            }
        })
        .map_err(|e| e.to_string())?;

    let today = chrono::Local::now().date_naive();
    let menu = ClockMenu::new(today, piece::skin("patina-clock"), requests);
    // At the clock, or on the bar's edge (ADR-0063).
    let (anchor, margin, exclusive_zone) = piece::opening(SIZE, GAP, Anchor::RIGHT);
    let options = Options {
        namespace: "patina-clock".into(),
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
                Event::Msg(Message::Agenda(from, items)) => host.shell.take_agenda(from, items),
                Event::Msg(Message::Close) => host.shell.close(),
                Event::Closed => return,
            }
            host.redraw_all();
        })
        .map_err(|e| e.to_string())?;
    host.run(event_loop)
}

/// Starts `command` among the compositor's processes (ADR-0045), or as our
/// own child, left to run after the menu has closed.
fn start(command: &str) {
    if patina_system::processes::spawn(command, &[]).is_ok() {
        return;
    }
    if let Err(e) = std::process::Command::new("sh")
        .arg("-c")
        .arg(command)
        .stdin(std::process::Stdio::null())
        .spawn()
    {
        eprintln!("patina-clock: can't run {command}: {e}");
    }
}
