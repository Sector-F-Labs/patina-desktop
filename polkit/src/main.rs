//! patina-polkit: the Patina desktop's polkit agent (ADR-0128). Kept running by
//! the compositor, it registers with polkit for this login session; when an
//! app or `pkexec` needs administrator rights, polkit asks it for the password.
//! It runs itself again as `--ask` to show the dialog (a patina-shell overlay).

mod agent;
mod dialog;

use std::process::ExitCode;

const USAGE: &str = "usage: patina-polkit

Registers as this session's polkit authentication agent, and asks for your
password in a dialog when polkit needs it. Run by the compositor; polkit
allows one agent per session, so it stops with an error if there already is
one (GNOME's, say).

  patina-polkit --ask --user NAME --message TEXT
      The dialog itself, started by the agent: talks to it over stdin and
      stdout. PATINA_SKIN, as patina-hud sets it, chooses the skin.";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        None => agent::run(),
        Some("-h" | "--help") => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Some("-V" | "--version") => {
            println!("patina-polkit {}", env!("CARGO_PKG_VERSION"));
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
            eprintln!("patina-polkit: {e}");
            ExitCode::FAILURE
        }
    }
}
