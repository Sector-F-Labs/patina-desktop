//! patina-screenshot: what the Print key opens (ADR-0091), a pill at the
//! bottom of the screen like dictation's (ADR-0073) with the choices: the
//! screen, the window that was focused, or a part of the screen to drag
//! out; or recording the screen or a part of it (ADR-0092). It prints the
//! one chosen and exits, or prints nothing on Escape; the compositor, which
//! started it, takes the screenshot once it's gone, so it's never in the
//! picture.
//!
//! `--recording` is the pill shown while a recording goes, and `--share`
//! the chooser xdg-desktop-portal-wlr runs when an app asks to share the
//! screen (ADR-0092).

mod hud;
mod indicator;
mod share;

use std::cell::Cell;
use std::process::ExitCode;
use std::rc::Rc;

use patina_shell::piece;
use patina_shell::{Anchor, Host, KeyboardInteractivity, Layer, Options};

use crate::hud::Hud;

const USAGE: &str = "usage: patina-screenshot [--no-window | --recording | --share]

Shows the screenshot choices at the bottom of the screen and prints the one
chosen: screen, window, selection, record-screen or record-selection;
nothing if it's closed with Escape. The compositor runs it on the Print key
and takes the screenshot or records. --no-window offers no window, for when
none is focused.

--recording shows that the screen is being recorded, until its stdin
closes; Stop prints stop. --share reads xdg-desktop-portal-wlr's sources on
stdin, a line each, and prints the one chosen.";

/// Between it and the screen's edge, past the bar if the bar is there.
const GAP: i32 = 24;

enum Mode {
    Choices { window: bool },
    Recording,
    Share,
}

fn main() -> ExitCode {
    let mut mode = Mode::Choices { window: true };
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--no-window" => mode = Mode::Choices { window: false },
            "--recording" => mode = Mode::Recording,
            "--share" => mode = Mode::Share,
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            "-V" | "--version" => {
                println!("patina-screenshot {}", env!("CARGO_PKG_VERSION"));
                return ExitCode::SUCCESS;
            }
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let printed = match mode {
        Mode::Choices { window } => run(window).map(|c| c.map(|c| c.word().to_string())),
        Mode::Recording => indicator::run().map(|stopped| stopped.then(|| "stop".to_string())),
        Mode::Share => share::run(),
    };
    match printed {
        Ok(Some(line)) => {
            println!("{line}");
            ExitCode::SUCCESS
        }
        Ok(None) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("patina-screenshot: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Shows the choices until one is made or it's closed; the surface is gone
/// by the time this returns.
fn run(window: bool) -> Result<Option<hud::Choice>, String> {
    let chosen = Rc::new(Cell::new(None));
    let hud = Hud::new(piece::skin("patina-screenshot"), window, chosen.clone());
    let options = Options {
        namespace: "patina-screenshot".into(),
        layer: Layer::Overlay,
        anchor: Anchor::BOTTOM,
        size: hud::SIZE,
        // 0: placed clear of the bar's exclusive zone, if it's on this edge.
        exclusive_zone: 0,
        margin: (GAP, GAP, GAP, GAP),
        keyboard: KeyboardInteractivity::Exclusive,
        per_output: false,
        toplevels: false,
        click_through: false,
        lock: false,
        window_fallback: false,
    };
    let (host, event_loop) = Host::connect(options, hud)?;
    host.run(event_loop)?;
    Ok(chosen.get())
}
