//! patina-emoji: the emoji picker (ADR-0120). A popup over the screen with a
//! search box and the emoji by category; Enter or a click picks one, and it
//! is typed into the window that had the focus, the way dictation's words are
//! (ADR-0071): `Commit` on `com.sectorflabs.Patina.Input`, which pastes it
//! through the clipboard. Running it while it's open closes it, so one key
//! toggles it.

mod data;
mod ui;

use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use patina_shell::piece::{self, Single};
use patina_shell::{Anchor, Host, KeyboardInteractivity, Layer, Options};

const USAGE: &str = "usage: patina-emoji

Opens the emoji picker. Type to search by name; Left, Right, Up and Down
choose, Enter or a click inserts the emoji into the window you were in, and
Escape closes. Tab and Shift+Tab move between the categories. Run it again
while it's open to close it.

The skin is the one named by PATINA_SKIN, or Default.";

/// The picker's size in points.
const SIZE: (u32, u32) = (400, 440);
/// After the picker is gone, before the text is sent: the focus returns to
/// the window it came from.
const FOCUS_BACK: Duration = Duration::from_millis(120);

const NAME: &str = "com.sectorflabs.Patina.Input";
const PATH: &str = "/com/sectorflabs/Patina/Input";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None => {}
        Some("-h" | "--help") => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Some("-V" | "--version") => {
            println!("patina-emoji {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Some(other) => {
            eprintln!("patina-emoji: unknown argument {other}\n{USAGE}");
            return ExitCode::from(2);
        }
    }
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("patina-emoji: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let (close, closing) = std::sync::mpsc::channel::<()>();
    // Already open: ask that one to close, and stop.
    let Some(_single) = Single::start("patina-emoji", "close", move |word| {
        word != "close" || close.send(()).is_ok()
    })?
    else {
        return Ok(());
    };

    let picked = Arc::new(Mutex::new(None));
    let picker = ui::Picker::new(
        piece::skin("patina-emoji"),
        data::load_recent(),
        picked.clone(),
    );
    let options = Options {
        namespace: "patina-emoji".into(),
        layer: Layer::Overlay,
        // No anchor: the compositor centres the surface.
        anchor: Anchor::empty(),
        size: SIZE,
        exclusive_zone: 0,
        margin: (0, 0, 0, 0),
        keyboard: KeyboardInteractivity::Exclusive,
        per_output: false,
        toplevels: false,
        click_through: false,
        lock: false,
        window_fallback: true,
    };
    let (host, event_loop) = Host::connect(options, picker)?;
    // A second run asks this one to close.
    let (send, receive) = patina_shell::calloop::channel::channel::<()>();
    std::thread::spawn(move || {
        while closing.recv().is_ok() {
            if send.send(()).is_err() {
                break;
            }
        }
    });
    event_loop
        .handle()
        .insert_source(receive, |event, _, host| {
            if let patina_shell::calloop::channel::Event::Msg(()) = event {
                host.shell.close();
            }
        })
        .map_err(|e| e.to_string())?;
    host.run(event_loop)?;

    // The picker is gone; the text goes where the focus is back.
    let emoji = picked.lock().ok().and_then(|mut p| p.take());
    if let Some(emoji) = emoji {
        data::remember(&emoji);
        std::thread::sleep(FOCUS_BACK);
        commit(&emoji)?;
    }
    Ok(())
}

/// Has the compositor type `text` into the focused window.
fn commit(text: &str) -> Result<(), String> {
    let bus = zbus::blocking::Connection::session().map_err(|e| e.to_string())?;
    bus.call_method(Some(NAME), PATH, Some(NAME), "Commit", &(text,))
        .map_err(|e| format!("Commit: {e}"))?;
    Ok(())
}
