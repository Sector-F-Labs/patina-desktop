//! patina-keyring: the Patina desktop's prompter for the keyring daemon. When
//! gnome-keyring (or anything speaking gcr's prompter protocol) needs a
//! password it can't get from logging in, it asks this, and a Patina dialog
//! opens. It runs itself again as `--ask` to show the dialog.

mod agent;
mod dialog;
mod exchange;

use std::process::ExitCode;

const USAGE: &str = "usage: patina-keyring

Takes over the keyring prompter's names on the session bus
(org.gnome.keyring.SystemPrompter and PrivatePrompter) and asks for a keyring's
password in a dialog when the daemon needs it. Run by the compositor; it
stops with an error if another prompter (GNOME's gcr-prompter) has the names.

  patina-keyring --ask [OPTIONS]
      The dialog itself, started by the prompter: it answers on stdout.
      PATINA_SKIN, as patina-hud sets it, chooses the skin.";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        None => agent::run(),
        Some("-h" | "--help") => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Some("-V" | "--version") => {
            println!("patina-keyring {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Some("--ask") => match dialog::Request::parse(&args[1..]) {
            Some(request) => dialog::run(request),
            None => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
        },
        Some(_) => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("patina-keyring: {e}");
            ExitCode::FAILURE
        }
    }
}
