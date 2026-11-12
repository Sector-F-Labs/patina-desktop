//! `patina-game-mode` (ADR-0134): frees the machine's resources while a game is
//! fullscreen. `--daemon` watches for a fullscreen window and does the work in
//! the config; `--toggle` asks a running daemon to switch game mode on or off by
//! hand, for a key binding to run.

mod config;
mod engine;

use std::time::{Duration, Instant};

/// What the compositor's Night Light says (ADR-0132) while a fullscreen window
/// has the keyboard and `[night_light] pause_in_fullscreen` is on.
const FULLSCREEN: &str = "A fullscreen window";

/// How often it looks.
const TICK: Duration = Duration::from_secs(1);

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("--daemon") => daemon(),
        Some("--toggle") => toggle(),
        Some("--toast") => {
            let text = std::env::args().nth(2).unwrap_or_default();
            if let Err(e) = patina_shell::toast::show("patina-game-mode", &text) {
                eprintln!("patina-game-mode: the notice: {e}");
            }
        }
        _ => {
            eprintln!(
                "usage: patina-game-mode --daemon | --toggle | --toast TEXT\n\n\
                 --daemon  watch for a fullscreen window and apply the settings\n\
                 --toggle  switch game mode on or off by hand (the daemon must be running)\n\
                 --toast   show TEXT at the top of the screen for a moment"
            );
            std::process::exit(2);
        }
    }
}

/// Whether the compositor says a fullscreen window has the keyboard.
fn fullscreen() -> bool {
    matches!(
        patina_system::night_light::status(),
        Ok(Some(status)) if status.inhibitors.iter().any(|i| i == FULLSCREEN)
    )
}

fn toggle() {
    let Some(path) = engine::toggle_request_path() else {
        eprintln!("patina-game-mode: no runtime folder");
        std::process::exit(1);
    };
    let made = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(&path, ""));
    if let Err(e) = made {
        eprintln!("patina-game-mode: {}: {e}", path.display());
        std::process::exit(1);
    }
}

fn daemon() {
    let mut config = config::load(&config::Config::default());
    let mut modified = config::modified();
    engine::take_over();
    // A marker an earlier run left isn't true now.
    engine::clear_marker();
    let mut mode = engine::GameMode::default();
    loop {
        let now = config::modified();
        if now != modified {
            modified = now;
            config = config::load(&config);
        }
        if engine::take_toggle_request() {
            mode.toggle();
        }
        mode.update(&config, fullscreen(), Instant::now());
        std::thread::sleep(TICK);
    }
}
