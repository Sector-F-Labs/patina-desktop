//! patina-menu: the desktop's menu, which the compositor opens at the
//! pointer on a right click on the desktop (ADR-0029). For now it holds the
//! skin menu, for the whole shell.

mod ui;
mod wallpapers;

use std::process::ExitCode;

use patina_shell::{Anchor, Host, KeyboardInteractivity, Layer, Options};

use crate::ui::DesktopMenu;

const USAGE: &str = "usage: patina-menu [--at X,Y]

Opens the desktop's menu at X,Y, in the screen's logical pixels (the
top-left corner without it), as patina-compositor does on a right click on
the desktop. Choosing a skin sets it in patina-bar's config, and the bar
and the other pieces follow it.";

fn main() -> ExitCode {
    let mut at = egui::Pos2::ZERO;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--at" => match args.next().as_deref().and_then(position) {
                Some(p) => at = p,
                None => return usage_error(),
            },
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            "-V" | "--version" => {
                println!("patina-menu {}", env!("CARGO_PKG_VERSION"));
                return ExitCode::SUCCESS;
            }
            _ => return usage_error(),
        }
    }
    match run(at) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("patina-menu: {e}");
            ExitCode::FAILURE
        }
    }
}

fn usage_error() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(2)
}

/// `X,Y` as a point.
fn position(text: &str) -> Option<egui::Pos2> {
    let (x, y) = text.split_once(',')?;
    Some(egui::pos2(x.trim().parse().ok()?, y.trim().parse().ok()?))
}

fn run(at: egui::Pos2) -> Result<(), String> {
    // The whole screen, transparent but for the menu: see ui.rs.
    let options = Options {
        namespace: "patina-menu".into(),
        layer: Layer::Overlay,
        anchor: Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
        size: (0, 0),
        exclusive_zone: -1,
        margin: (0, 0, 0, 0),
        keyboard: KeyboardInteractivity::Exclusive,
        per_output: false,
        toplevels: false,
        click_through: false,
        lock: false,
    };
    let (host, event_loop) = Host::connect(options, DesktopMenu::new(at))?;
    host.run(event_loop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_positions() {
        assert_eq!(position("120,48"), Some(egui::pos2(120.0, 48.0)));
        assert_eq!(position("10.5, 3"), Some(egui::pos2(10.5, 3.0)));
        assert_eq!(position("10"), None);
        assert_eq!(position("a,b"), None);
    }
}
