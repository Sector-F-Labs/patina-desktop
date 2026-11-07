//! patina-displays: what the display key (Ctrl+Super+P) and plugging a
//! monitor in open (ADR-0121): a pill like the screenshot's, listing the
//! monitors and offering to extend the desktop across them or mirror it. The
//! choice is made through the compositor's `com.sectorflabs.Patina.Displays`;
//! extending with a monitor never placed before then opens Settings at the
//! Displays page, to say where it sits.

mod hud;

use std::cell::Cell;
use std::process::{Command, ExitCode};
use std::rc::Rc;

use patina_shell::piece;
use patina_shell::{Anchor, Host, KeyboardInteractivity, Layer, Options};
use patina_system::displays::{self, Mode, Monitor};

use crate::hud::Hud;

const USAGE: &str = "usage: patina-displays

Shows the connected monitors and offers to extend the desktop across them or
mirror it, or leave it be: arrows or Tab move, Enter or 1 to 3 choose, Escape
closes. The compositor opens it on Ctrl+Super+P and when a monitor is plugged
in, which stays off until it's chosen for. Extending with a monitor not placed
before opens Settings' Displays page.

The skin is the one named by PATINA_SKIN, or Default.";

/// Between it and the screen's edge, past the bar if the bar is there.
const GAP: i32 = 24;

fn main() -> ExitCode {
    match std::env::args().nth(1).as_deref() {
        None => {}
        Some("-h" | "--help") => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Some("-V" | "--version") => {
            println!("patina-displays {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Some(other) => {
            eprintln!("patina-displays: unknown argument {other}\n{USAGE}");
            return ExitCode::from(2);
        }
    }
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("patina-displays: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Whether choosing `mode` should send the user to Settings: extending onto
/// a monitor that has never been placed (the primary has no place).
fn needs_placing(mode: Mode, monitors: &[Monitor]) -> bool {
    mode == Mode::Extend && monitors.iter().any(|m| !m.known && !m.primary)
}

/// The monitors and the mode, asking again for a few seconds: just after a
/// monitor is plugged in the compositor is busy with it.
fn ask() -> Result<(Vec<Monitor>, Mode), String> {
    let mut tries = 0;
    loop {
        match displays::list().and_then(|m| Ok((m, displays::mode()?))) {
            Ok(answer) => return Ok(answer),
            Err(e) if tries >= 12 => return Err(e),
            Err(_) => {
                tries += 1;
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
        }
    }
}

fn run() -> Result<(), String> {
    let (monitors, current) = ask()?;
    let chosen = Rc::new(Cell::new(None));
    let hud = Hud::new(
        piece::skin("patina-displays"),
        &monitors,
        current,
        chosen.clone(),
    );
    let options = Options {
        namespace: "patina-displays".into(),
        layer: Layer::Overlay,
        anchor: Anchor::BOTTOM,
        size: hud::SIZE,
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
    let Some(mode) = chosen.get() else {
        return Ok(());
    };
    // Doing nothing about a monitor just plugged in leaves it off.
    displays::set_mode(mode)?;
    if needs_placing(mode, &monitors) {
        Command::new("patina-settings")
            .args(["--page", "displays"])
            .spawn()
            .map_err(|e| format!("can't open Settings: {e}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor(known: bool) -> Monitor {
        Monitor {
            name: "DP-3".into(),
            id: "x".into(),
            make: String::new(),
            model: String::new(),
            width: 1920,
            height: 1080,
            x: 0,
            y: 0,
            logical_width: 1920,
            logical_height: 1080,
            scale: 1.0,
            primary: false,
            known,
            active: true,
            to: String::new(),
            side: None,
            offset: 0,
        }
    }

    #[test]
    fn only_extending_onto_a_new_monitor_goes_to_settings() {
        let new = [monitor(true), monitor(false)];
        let old = [monitor(true), monitor(true)];
        assert!(needs_placing(Mode::Extend, &new));
        assert!(!needs_placing(Mode::Mirror, &new));
        assert!(!needs_placing(Mode::Extend, &old));
    }
}
