//! patina-notify: the Patina desktop's notification daemon (ADR-0141). Kept
//! running by the compositor, it owns `org.freedesktop.Notifications` and
//! draws what arrives as a column of cards at the right edge, which keeps its
//! space so tiled windows make room.

mod column;
mod config;
mod daemon;
mod fullscreen;
mod store;

use std::process::ExitCode;

const USAGE: &str = "usage: patina-notify

Shows notifications as cards down the right edge of the screen, one under
another, each with a close button, until they are dismissed. Tiled windows
make room while any are showing. Run by the compositor; stops with an error if
another notification daemon (GNOME's, say) already has the name.

  ~/.config/patina-notify/config.toml
      enabled = false             turns them off
      hide_in_fullscreen = false  keeps them showing over a fullscreen window's
                                  space; by default they wait until it's left";

fn main() -> ExitCode {
    match std::env::args().nth(1).as_deref() {
        None => {}
        Some("-h" | "--help") => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Some("-V" | "--version") => {
            println!("patina-notify {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Some(_) => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    }
    match column::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("patina-notify: {e}");
            ExitCode::FAILURE
        }
    }
}
