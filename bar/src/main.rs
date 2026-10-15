//! patina-bar: a status bar for dwl, in the Patina style.
//!
//! Run it as dwl's status command, `dwl -s patina-bar`, and it reads dwl's
//! status lines from stdin.

mod bar;
mod config;
mod dwl;
mod launcher;
mod tray;

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
        margin: (0, 0, 0, 0),
        keyboard: KeyboardInteractivity::None,
        per_output: true,
        toplevels: true,
        lock: false,
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

    let uses_tray = host
        .shell
        .config
        .right
        .iter()
        .chain(&host.shell.config.left)
        .chain(&host.shell.config.center)
        .any(|m| m == "tray");
    // The tray's speaker follows the volume, read once a second.
    if host
        .shell
        .config
        .right
        .iter()
        .chain(&host.shell.config.left)
        .chain(&host.shell.config.center)
        .any(|m| m == "tray")
    {
        let (levels, volume) = channel::channel::<patina_shell::audio::Volume>();
        std::thread::Builder::new()
            .name("volume".into())
            .spawn(move || {
                loop {
                    if let Some(v) = patina_shell::audio::get()
                        && levels.send(v).is_err()
                    {
                        return;
                    }
                    std::thread::sleep(Duration::from_secs(1));
                }
            })
            .map_err(|e| e.to_string())?;
        handle
            .insert_source(volume, |event, _, host| {
                if let Event::Msg(v) = event
                    && host.shell.volume != Some(v)
                {
                    host.shell.volume = Some(v);
                    host.redraw_all();
                }
            })
            .map_err(|e| e.to_string())?;
    }

    // The tray's Wi-Fi icon follows the signal, read every few seconds.
    if uses_tray {
        let (signals, signal) = channel::channel::<Option<Option<u8>>>();
        std::thread::Builder::new()
            .name("wifi".into())
            .spawn(move || {
                let Ok(nm) = patina_shell::network::NetworkManager::new() else {
                    return;
                };
                loop {
                    if signals.send(nm.signal()).is_err() {
                        return;
                    }
                    std::thread::sleep(Duration::from_secs(3));
                }
            })
            .map_err(|e| e.to_string())?;
        handle
            .insert_source(signal, |event, _, host| {
                if let Event::Msg(s) = event
                    && host.shell.wifi != s
                {
                    host.shell.wifi = s;
                    host.redraw_all();
                }
            })
            .map_err(|e| e.to_string())?;
    }

    // The tray's battery, read every few seconds, so plugging in shows soon.
    if uses_tray {
        let (readings, battery) = channel::channel::<Option<patina_shell::power::Battery>>();
        std::thread::Builder::new()
            .name("battery".into())
            .spawn(move || {
                loop {
                    if readings.send(patina_shell::power::read()).is_err() {
                        return;
                    }
                    std::thread::sleep(Duration::from_secs(5));
                }
            })
            .map_err(|e| e.to_string())?;
        handle
            .insert_source(battery, |event, _, host| {
                if let Event::Msg(b) = event
                    && host.shell.battery != b
                {
                    host.shell.battery = b;
                    host.redraw_all();
                }
            })
            .map_err(|e| e.to_string())?;
    }

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
