//! patina-hud: a status bar for dwl, in the Patina style.
//!
//! Run it as dwl's status command, `dwl -s patina-hud`, and it reads dwl's
//! status lines from stdin.

mod bar;
mod config;
mod dwl;
mod gaps;
mod hide;
mod icons;
mod launcher;
mod menu;
mod pins;
mod split;
mod tooltip;
mod tray;

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, SystemTime};

use patina_shell::calloop::{
    channel::{self, Event},
    timer::{TimeoutAction, Timer},
};
use patina_shell::{Anchor, Host, KeyboardInteractivity, Layer, Options};

use crate::config::{Layout, Position, Visibility};
use crate::split::Section;

const USAGE: &str = "usage: patina-hud [--toggle] [--peek launcher] [--open launcher] [-c config.toml] [--section launcher|windows|tray]

A status bar for dwl. Run it as dwl's status command, so it reads dwl's
status from stdin:

    dwl -s patina-hud

The config defaults to ~/.config/patina-hud/config.toml. With layout =
\"split\" in it (experimental), the bar draws the window list and runs
the launcher and the tray as `patina-hud --section`, each on its own.";

fn main() -> ExitCode {
    let mut config_path = config::default_path();
    let mut section_arg = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--section" => match args.next().as_deref().and_then(Section::from_name) {
                Some(s) => section_arg = Some(s),
                None => return usage_error(),
            },
            "-c" | "--config" => match args.next() {
                Some(p) => config_path = Some(PathBuf::from(p)),
                None => return usage_error(),
            },
            "--toggle" => {
                return match hide::toggle() {
                    Ok(hidden) => {
                        println!("{}", if hidden { "hidden" } else { "shown" });
                        ExitCode::SUCCESS
                    }
                    Err(e) => {
                        eprintln!("patina-hud: {e}");
                        ExitCode::FAILURE
                    }
                };
            }
            "--open" => {
                return match args.next().as_deref() {
                    Some("launcher") => open_launcher(config_path.as_deref()),
                    _ => usage_error(),
                };
            }
            "--peek" => {
                return match args.next().map(|n| hide::peek(&n)) {
                    Some(Ok(())) => ExitCode::SUCCESS,
                    Some(Err(e)) => {
                        eprintln!("patina-hud: {e}");
                        ExitCode::FAILURE
                    }
                    None => usage_error(),
                };
            }
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            "-V" | "--version" => {
                println!("patina-hud {}", env!("CARGO_PKG_VERSION"));
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
    let gaps = gaps::read();
    // Split, this one draws the window list and runs the other parts.
    let section = match (config.layout, section_arg) {
        // The launcher button left out: a stray one goes.
        (Layout::Split, Some(s))
            if s != Section::Windows
                && !matches!(s, Section::Reserve(_))
                && s.mode(&config) == Visibility::Hidden =>
        {
            return ExitCode::SUCCESS;
        }
        (Layout::Split, Some(s)) => Some(s),
        (Layout::Split, None) => Some(Section::Windows),
        // A part left over from a split bar that's whole again goes.
        (Layout::Bar, Some(_)) => return ExitCode::SUCCESS,
        (Layout::Bar, None) => None,
    };
    let parts = if config.layout == Layout::Split && section_arg.is_none() {
        let mut parts = Vec::new();
        for section in [Section::Tray, Section::Launcher] {
            if section.mode(&config) != Visibility::Hidden {
                parts.push(section);
            }
        }
        if split::media_apart(&config).is_some()
            && Section::Media.mode(&config) != Visibility::Hidden
        {
            parts.push(Section::Media);
        }
        parts.extend(split::reserved(&config).into_iter().map(Section::Reserve));
        start_parts(&parts, config_path.as_deref())
    } else {
        Vec::new()
    };
    let place = split::placement(section, &config, gaps);
    let options = Options {
        namespace: match section {
            Some(s) => format!("patina-hud-{}", s.name()),
            None => "patina-hud".into(),
        },
        layer: Layer::Top,
        anchor: place.anchor,
        size: place.size,
        exclusive_zone: place.exclusive_zone,
        margin: place.margin,
        keyboard: KeyboardInteractivity::None,
        per_output: true,
        toplevels: matches!(section, None | Some(Section::Windows)),
        // A reserve only keeps an edge's space.
        click_through: matches!(section, Some(Section::Reserve(_))),
        lock: false,
        window_fallback: false,
    };
    let (mut bar, skin_warnings) = bar::Bar::new(config);
    bar.gaps = gaps;
    bar.section = section;
    bar.place = Some(place);
    bar.all_hidden = hide::hidden();
    bar.config_path = config_path.clone();
    warnings.extend(skin_warnings);
    for w in &warnings {
        eprintln!("patina-hud: {w}");
    }

    match run(options, bar, config_path, parts, section_arg.is_some()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("patina-hud: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Where the bar sits: across the top or the bottom.
fn anchor(position: Position) -> Anchor {
    let edge = match position {
        Position::Top => Anchor::TOP,
        Position::Bottom => Anchor::BOTTOM,
    };
    edge | Anchor::LEFT | Anchor::RIGHT
}

/// Starts the split bar's other parts (the launcher, the tray, and the
/// reserves keeping their edges) as `patina-hud --section`, each with its
/// standard input a pipe from this one: they end when it closes, as this
/// process ends or restarts.
fn start_parts(parts: &[Section], config_path: Option<&Path>) -> Vec<std::process::Child> {
    let program = std::env::args()
        .next()
        .unwrap_or_else(|| "patina-hud".into());
    parts
        .iter()
        .filter_map(|section| {
            let mut command = std::process::Command::new(&program);
            command
                .args(["--section", &section.name()])
                .stdin(std::process::Stdio::piped());
            if let Some(path) = config_path {
                command.arg("-c").arg(path);
            }
            match command.spawn() {
                Ok(child) => Some(child),
                Err(e) => {
                    eprintln!("patina-hud: can't start the {}: {e}", section.name());
                    None
                }
            }
        })
        .collect()
}

/// Starts this bar again in its place, as its layout changed: the parts it
/// ran end as their pipes close, and the new one starts its own.
fn restart(parts: &mut Vec<std::process::Child>) -> ! {
    use std::os::unix::process::CommandExt;
    // Each part ends as its pipe closes; waited for, so none is left a
    // zombie of the new bar, which never knew it.
    for mut part in parts.drain(..) {
        drop(part.stdin.take());
        let _ = part.wait();
    }
    let mut args = std::env::args();
    let program = args.next().unwrap_or_else(|| "patina-hud".into());
    let e = std::process::Command::new(program).args(args).exec();
    eprintln!("patina-hud: can't restart: {e}");
    std::process::exit(1)
}

/// Runs the launcher as its button would, from the button's place, as for
/// the Super key (the compositor runs this); the button comes out for a
/// moment if it hides.
fn open_launcher(config_path: Option<&Path>) -> ExitCode {
    let config = config_path.map_or_else(config::Config::default, |p| config::load(p).0);
    let gaps = gaps::read();
    let place = split::placement(Some(Section::Launcher), &config, gaps);
    let opening = match config.launcher_at {
        Some(spot) => {
            let (w, h) = (place.size.0 as f32, place.size.1 as f32);
            let side = split::thickness(Section::Launcher, &config) as f32 / 2.0;
            let at = place.popup(egui::pos2(side, side), egui::vec2(w, h), None);
            launcher::Opening {
                edge: if spot.y() == config::Along::Start {
                    "top"
                } else {
                    "bottom"
                },
                at: Some(at.env()),
            }
        }
        // No button: open where the bar would have it.
        None => launcher::Opening {
            edge: if config.position == Position::Top {
                "top"
            } else {
                "bottom"
            },
            at: None,
        },
    };
    let _ = hide::peek("launcher");
    let _ = launcher::run(&config.launcher, &config.skin, &opening).join();
    ExitCode::SUCCESS
}

fn usage_error() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(2)
}

fn run(
    options: Options,
    bar: bar::Bar,
    config_path: Option<PathBuf>,
    mut parts: Vec<std::process::Child>,
    is_part: bool,
) -> Result<(), String> {
    let (mut host, event_loop) = Host::connect(options, bar)?;
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
                    // A part of a split bar ends with the bar that ran it.
                    Ok(0) | Err(_) if is_part => std::process::exit(0),
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

    // The window tabs' apps' names and icons, found on a thread of their
    // own.
    let (found, answers) = channel::channel::<icons::Found>();
    host.shell.ask_app = Some(icons::spawn(move |answer| found.send(answer).is_ok()));
    handle
        .insert_source(answers, |event, _, host| {
            if let Event::Msg(answer) = event {
                host.shell.app_found(answer);
                host.redraw_all();
            }
        })
        .map_err(|e| e.to_string())?;

    // Windows moved along the tabs (ADR-0080): by the compositor, which
    // signals each, or by the bar itself where nothing keeps the order.
    let (moved, moves) = channel::channel::<(usize, usize)>();
    if matches!(host.shell.section, None | Some(Section::Windows)) {
        let signalled = moved.clone();
        std::thread::Builder::new()
            .name("tab-moves".into())
            .spawn(move || patina_system::pins::watch_moves(|m| signalled.send(m).is_ok()))
            .map_err(|e| e.to_string())?;
    }
    host.shell.moves = Some(moved);
    handle
        .insert_source(moves, |event, _, host| {
            if let Event::Msg((from, to)) = event {
                host.shell.moved(from, to);
                host.redraw_all();
            }
        })
        .map_err(|e| e.to_string())?;

    // What's chosen in the window tabs' menus, opened in patina-menu.
    let (chosen, tab_menus) = channel::channel::<menu::Chosen>();
    host.shell.tab_menu = Some(chosen);
    handle
        .insert_source(tab_menus, |event, _, host| {
            if let Event::Msg(chosen) = event {
                host.shell.tab_menu_chosen(chosen);
            }
        })
        .map_err(|e| e.to_string())?;

    let modules: Vec<String> = match host.shell.section {
        Some(section) => split::modules(section, &host.shell.config),
        None => {
            let c = &host.shell.config;
            c.left
                .iter()
                .chain(&c.center)
                .chain(&c.right)
                .cloned()
                .collect()
        }
    };
    let uses_tray = modules.iter().any(|m| m == "tray");
    let uses_media = modules.iter().any(|m| m == "media");
    // The tray's speaker follows the volume, read once a second.
    if uses_tray {
        let (levels, volume) = channel::channel::<patina_system::audio::Volume>();
        std::thread::Builder::new()
            .name("volume".into())
            .spawn(move || {
                loop {
                    if let Some(v) = patina_system::audio::get()
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

    // The media controls follow the player playing (ADR-0101), asked of the
    // session bus twice a second so a click shows at once.
    if uses_media {
        let (players, media) = channel::channel::<Option<patina_system::mpris::Player>>();
        std::thread::Builder::new()
            .name("media".into())
            .spawn(move || {
                let mut bus = None;
                loop {
                    if bus.is_none() {
                        bus = patina_system::mpris::connect().ok();
                    }
                    if let Some(b) = &bus {
                        let current = patina_system::mpris::current(b).unwrap_or(None);
                        if players.send(current).is_err() {
                            return;
                        }
                    }
                    std::thread::sleep(Duration::from_millis(500));
                }
            })
            .map_err(|e| e.to_string())?;
        handle
            .insert_source(media, |event, _, host| {
                if let Event::Msg(p) = event
                    && host.shell.media != p
                {
                    host.shell.media = p;
                    host.redraw_all();
                }
            })
            .map_err(|e| e.to_string())?;
    }

    // Apps' status icons (ADR-0064): the bar is the tray they register
    // with, wherever its tray is.
    if uses_tray {
        let (sent, items) = channel::channel::<Vec<patina_system::sni::Item>>();
        let side = match host.shell.section {
            Some(_) => split::thickness(Section::Tray, &host.shell.config) as f32,
            None => host.shell.config.height.max(1) as f32,
        };
        let pixels = (side - 2.0 * tray::padding(side)).round().max(8.0) as u32;
        match patina_system::sni::start(pixels, move |list| sent.send(list).is_ok()) {
            Ok(handle) => host.shell.tray = Some(handle),
            Err(e) => eprintln!("patina-hud: no tray for apps' icons: {e}"),
        }
        handle
            .insert_source(items, |event, _, host| {
                if let Event::Msg(list) = event {
                    host.shell.tray_items = list;
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
                let Ok(nm) = patina_system::network::open() else {
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

    // The tray's keyboard layout (ADR-0068), as the compositor announces it.
    if uses_tray {
        let (statuses, keyboard) = channel::channel::<Option<patina_system::keyboard::Status>>();
        std::thread::Builder::new()
            .name("keyboard".into())
            .spawn(move || patina_system::keyboard::watch(|s| statuses.send(s).is_ok()))
            .map_err(|e| e.to_string())?;
        handle
            .insert_source(keyboard, |event, _, host| {
                if let Event::Msg(s) = event
                    && host.shell.keyboard != s
                {
                    host.shell.keyboard = s;
                    host.redraw_all();
                }
            })
            .map_err(|e| e.to_string())?;
    }

    // The tray's battery, read every few seconds, so plugging in shows soon.
    if uses_tray {
        let (readings, battery) = channel::channel::<Option<patina_system::power::Battery>>();
        std::thread::Builder::new()
            .name("battery".into())
            .spawn(move || {
                loop {
                    if readings.send(patina_system::power::read()).is_err() {
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

    // The clock changes on its own; redraw on each second. Then too, look
    // for changes to the config file, as the desktop's menu and Settings
    // make them: the skin, the window tabs and the edge follow at once. So
    // do the gaps, from the compositor's config.
    let mut seen = config_path.as_deref().and_then(modified);
    let gaps_path = gaps::path();
    let mut gaps_seen = gaps_path.as_deref().and_then(modified);
    handle
        .insert_source(
            Timer::from_duration(until_next_second()),
            move |_, _, host| {
                let split = host.shell.section.is_some();
                if let Some(path) = &config_path {
                    let now = modified(path);
                    if now != seen {
                        seen = now;
                        let (config, _) = config::load(path);
                        let current = &host.shell.config;
                        let relaid = config.layout != current.layout
                            || (config.layout == Layout::Split
                                && (config.windows_at != current.windows_at
                                    || config.launcher_at != current.launcher_at
                                    || config.tray_at != current.tray_at
                                    || config.media_at != current.media_at
                                    || config.launcher_mode != current.launcher_mode
                                    || config.windows_mode != current.windows_mode
                                    || config.tray_mode != current.tray_mode
                                    || config.media_mode != current.media_mode
                                    || config.height != current.height
                                    || config.launcher_height != current.launcher_height
                                    || config.windows_height != current.windows_height
                                    || config.tray_height != current.tray_height));
                        // The parts go with the bar that ran them.
                        if relaid && !is_part {
                            restart(&mut parts);
                        }
                        host.shell.config.window_tabs = config.window_tabs;
                        host.shell.set_pinned(config.pinned.clone());
                        if !split && config.position != host.shell.config.position {
                            host.shell.config.position = config.position;
                            host.set_anchor(anchor(config.position));
                            let top = config.position == Position::Top;
                            host.set_margin(host.shell.gaps.margin(top));
                        }
                        let (changed, warnings) = host.shell.set_skin(&config.skin);
                        for w in warnings {
                            eprintln!("patina-hud: {w}");
                        }
                        if changed {
                            host.restyle();
                        }
                    }
                }
                let now = gaps_path.as_deref().and_then(modified);
                if now != gaps_seen {
                    gaps_seen = now;
                    let gaps = gaps::read();
                    if gaps != host.shell.gaps {
                        if split && !is_part {
                            restart(&mut parts);
                        }
                        host.shell.gaps = gaps;
                        if !split {
                            let top = host.shell.config.position == Position::Top;
                            host.set_margin(gaps.margin(top));
                        }
                    }
                }
                host.redraw_all();
                TimeoutAction::ToDuration(until_next_second())
            },
        )
        .map_err(|e| e.to_string())?;

    // Super+H and a peek's ask: look for them a few times a second.
    let name = host.shell.section.map(|s| s.name());
    let mut peeked = name.as_deref().and_then(hide::peeked);
    handle
        .insert_source(
            Timer::from_duration(Duration::from_millis(100)),
            move |_, _, host| {
                let hidden = hide::hidden();
                let mut redraw = hidden != host.shell.all_hidden;
                host.shell.all_hidden = hidden;
                if let Some(name) = &name
                    && let Some(at) = hide::peeked(name)
                    && peeked != Some(at)
                {
                    peeked = Some(at);
                    host.shell.peek = true;
                    redraw = true;
                }
                if redraw {
                    host.redraw_all();
                }
                TimeoutAction::ToDuration(Duration::from_millis(100))
            },
        )
        .map_err(|e| e.to_string())?;

    host.run(event_loop)
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn until_next_second() -> Duration {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    Duration::from_nanos(1_000_000_000 - u64::from(now.subsec_nanos())) + Duration::from_millis(5)
}
