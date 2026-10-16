//! patina-night-light: Night Light from the command line (ADR-0039), a
//! client of the compositor's `com.sectorflabs.Patina.NightLight`.

use std::process::{Command, ExitCode};

use patina_system::night_light;

const USAGE: &str = "usage: patina-night-light [status]
       patina-night-light on|off|auto
       patina-night-light inhibit [--reason TEXT] -- COMMAND [ARG ...]

status   how Night Light stands (the default)
on, off  turn it on or off until the schedule next changes
auto     follow the schedule again
inhibit  hold it off while COMMAND runs, as a photo editor needs";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        None | Some("status") if args.len() <= 1 => status(),
        Some(state @ ("on" | "off" | "auto")) if args.len() == 1 => {
            night_light::set_override(state)
        }
        Some("inhibit") => return inhibit(&args[1..]),
        Some("-h" | "--help") => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Some("-V" | "--version") => {
            println!("patina-night-light {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("patina-night-light: {e}");
            ExitCode::FAILURE
        }
    }
}

fn status() -> Result<(), String> {
    let Some(s) = night_light::status()? else {
        return Err("Night Light isn't served here: Patina isn't running on the hardware".into());
    };
    if !s.available {
        println!("unavailable: the screen's driver has no gamma ramp");
        return Ok(());
    }
    println!("{}", if s.active { "on" } else { "off" });
    println!("temperature: {} K", s.temperature);
    println!("override: {}", s.overridden);
    for reason in &s.inhibitors {
        println!("inhibited: {reason}");
    }
    Ok(())
}

/// Holds Night Light off while a command runs, and exits as it does.
fn inhibit(args: &[String]) -> ExitCode {
    let (mut reason, mut rest) = (None, args);
    if rest.first().map(String::as_str) == Some("--reason") && rest.len() >= 2 {
        reason = Some(rest[1].clone());
        rest = &rest[2..];
    }
    let Some(command) = rest
        .strip_prefix(&["--".to_string()])
        .filter(|c| !c.is_empty())
    else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let reason = reason.unwrap_or_else(|| command[0].clone());
    let held = match night_light::inhibit(&reason) {
        Ok(h) => Some(h),
        Err(e) => {
            // The command still runs; there's just nothing to hold off.
            eprintln!("patina-night-light: not inhibiting: {e}");
            None
        }
    };
    let status = Command::new(&command[0]).args(&command[1..]).status();
    drop(held);
    match status {
        Ok(s) => ExitCode::from(s.code().unwrap_or(1).clamp(0, 255) as u8),
        Err(e) => {
            eprintln!("patina-night-light: {}: {e}", command[0]);
            ExitCode::from(127)
        }
    }
}
