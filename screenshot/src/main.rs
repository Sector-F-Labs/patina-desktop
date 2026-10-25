//! patina-screenshot: what the Print key opens (ADR-0091), a pill at the
//! bottom of the screen like dictation's (ADR-0073) with three choices:
//! the screen, the window that was focused, or a part of the screen to
//! drag out. It prints the one chosen (`screen`, `window` or `selection`)
//! and exits, or prints nothing on Escape; the compositor, which started
//! it, takes the screenshot once it's gone, so it's never in the picture.

mod hud;

use std::cell::Cell;
use std::process::ExitCode;
use std::rc::Rc;

use patina_shell::piece;
use patina_shell::{Anchor, Host, KeyboardInteractivity, Layer, Options};

use crate::hud::Hud;

const USAGE: &str = "usage: patina-screenshot [--no-window]

Shows the screenshot choices at the bottom of the screen and prints the one
chosen: screen, window or selection; nothing if it's closed with Escape.
The compositor runs it on the Print key and takes the screenshot.
--no-window offers no window, for when none is focused.";

/// Between it and the screen's edge, past the bar if the bar is there.
const GAP: i32 = 24;

fn main() -> ExitCode {
    let mut window = true;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--no-window" => window = false,
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
    match run(window) {
        Ok(Some(choice)) => {
            println!("{}", choice.word());
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
