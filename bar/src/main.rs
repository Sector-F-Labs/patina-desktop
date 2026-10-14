//! patina-bar: a status bar for dwl, in the Patina style.
//!
//! Run it as dwl's status command, `dwl -s patina-bar`, and it reads dwl's
//! status lines from stdin.

mod bar;
mod config;
mod dwl;
mod launcher;

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use patina_shell::calloop::{
    channel::{self, Event},
    timer::{TimeoutAction, Timer},
};
use patina_shell::{Anchor, Host, KeyboardInteractivity, Layer, Options};

use crate::config::Position;

const USAGE: &str = "usage: patina-bar [-c config.toml]

A status bar for dwl. Run it as dwl's status command, so it reads dwl's
status from stdin:

    dwl -s patina-bar

The config defaults to ~/.config/patina-bar/config.toml.";

fn main() -> ExitCode {
    let mut config_path = config::default_path();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-c" | "--config" => match args.next() {
                Some(p) => config_path = Some(PathBuf::from(p)),
                None => return usage_error(),
            },
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            "-V" | "--version" => {
                println!("patina-bar {}", env!("CARGO_PKG_VERSION"));
                return ExitCode::SUCCESS;
            }
            _ => return usage_error(),
        }
    }

    let (config, mut warnings) = match &config_path {
        Some(path) => {
            let (c, w) = config::load(path);
            (
                c,
                w.into_iter()
                    .map(|w| format!("{}: {w}", path.display()))
                    .collect(),
            )
        }
        None => (config::Config::default(), Vec::new()),
    };
    let height = config.height.max(1);
    let edge = match config.position {
        Position::Top => Anchor::TOP,
        Position::Bottom => Anchor::BOTTOM,
    };
    let options = Options {
        namespace: "patina-bar".into(),
        layer: Layer::Top,
        anchor: edge | Anchor::LEFT | Anchor::RIGHT,
        size: (0, height),
        exclusive_zone: height as i32,
        keyboard: KeyboardInteractivity::None,
        per_output: true,
    };
    let (bar, skin_warnings) = bar::Bar::new(config);
    warnings.extend(skin_warnings);
    for w in &warnings {
        eprintln!("patina-bar: {w}");
    }

    match run(options, bar) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("patina-bar: {e}");
            ExitCode::FAILURE
        }
    }
}

fn usage_error() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(2)
}

fn run(options: Options, bar: bar::Bar) -> Result<(), String> {
    let (host, event_loop) = Host::connect(options, bar)?;
    let handle = event_loop.handle();

    // dwl blocks when its status pipe fills up, so stdin is read on a thread
    // of its own that never waits on drawing.
    let (lines, receiver) = channel::channel::<String>();
    std::thread::Builder::new()
        .name("dwl-status".into())
        .spawn(move || {
            let mut stdin = BufReader::new(std::io::stdin().lock());
            let mut line = Vec::new();
            loop {
                line.clear();
                match stdin.read_until(b'\n', &mut line) {
                    Ok(0) | Err(_) => return,
                    // Titles are whatever the window set, so not always UTF-8.
                    Ok(_) => {
                        if lines
                            .send(String::from_utf8_lossy(&line).into_owned())
                            .is_err()
                        {
                            return;
                        }
                    }
                }
            }
        })
        .map_err(|e| e.to_string())?;
    handle
        .insert_source(receiver, |event, _, host| {
            if let Event::Msg(line) = event
                && host.shell.status.apply(&line)
            {
                host.redraw_all();
            }
        })
        .map_err(|e| e.to_string())?;

    // The clock and load change on their own; redraw on each second.
    handle
        .insert_source(Timer::from_duration(until_next_second()), |_, _, host| {
            host.redraw_all();
            TimeoutAction::ToDuration(until_next_second())
        })
        .map_err(|e| e.to_string())?;

    host.run(event_loop)
}

fn until_next_second() -> Duration {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    Duration::from_nanos(1_000_000_000 - u64::from(now.subsec_nanos())) + Duration::from_millis(5)
}
