//! patina-launcher: the Patina desktop's app launcher. It lists the apps
//! installed through `.desktop` files, filters them as you type, and starts
//! the one you pick. Running it while it's open closes it, so one button
//! toggles it.

mod apps;
mod folders;
mod icons;
mod ui;

use std::process::ExitCode;

use patina_shell::calloop::channel::{self, Event};
use patina_shell::piece::{self, Single};
use patina_shell::{Anchor, Host, KeyboardInteractivity, Layer, Options};

const USAGE: &str = "usage: patina-launcher [--position bar|center] [--search bottom|top]

Lists the installed apps, filters them as you type and starts the one you
pick. Run it again while it's open to close it.

  --position bar      open at the bar's button, like a menu (the default):
                      at the left end of the edge named by PATINA_BAR_EDGE
                      (top or bottom, as patina-bar sets; bottom by default)
  --position center   open in the middle of the screen, as Spotlight does
  --search bottom     the search box below the list (the default)
  --search top        the search box above the list

The skin is the one named by PATINA_SKIN, or Default.";

/// The launcher's size in points.
const SIZE: (u32, u32) = (420, 500);
/// Its distance from the bar and the screen's left edge, as a menu sits
/// just off the button that opened it.
const GAP: i32 = 4;

/// Where the launcher opens.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Position {
    /// At the bar's button, or the bar's edge.
    #[default]
    Bar,
    /// In the middle of the screen.
    Center,
}

/// What the command line asked for.
#[derive(Debug, Default, PartialEq)]
struct Args {
    position: Position,
    search: ui::Search,
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut parsed = Args::default();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let (name, inline) = match arg.split_once('=') {
            Some((name, value)) => (name.to_owned(), Some(value.to_owned())),
            None => (arg, None),
        };
        if name != "--position" && name != "--search" {
            return Err(format!("unknown argument {name}"));
        }
        let value = inline
            .or_else(|| args.next())
            .ok_or_else(|| format!("{name} needs a value"))?;
        match (name.as_str(), value.as_str()) {
            ("--position", "bar") => parsed.position = Position::Bar,
            ("--position", "center") => parsed.position = Position::Center,
            ("--search", "bottom") => parsed.search = ui::Search::Bottom,
            ("--search", "top") => parsed.search = ui::Search::Top,
            _ => return Err(format!("{value} isn't a value for {name}")),
        }
    }
    Ok(parsed)
}

enum Message {
    Icon(usize, egui::ColorImage),
    Close,
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("patina-launcher {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    let args = match parse_args(args) {
        Ok(args) => args,
        Err(e) => {
            eprintln!("patina-launcher: {e}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match run(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("patina-launcher: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Args) -> Result<(), String> {
    let (send, receive) = channel::channel::<Message>();
    let closer = send.clone();
    // Already open: ask that one to close, and stop.
    let Some(_single) = Single::start("patina-launcher", "close", move |word| {
        word != "close" || closer.send(Message::Close).is_ok()
    })?
    else {
        return Ok(());
    };

    let apps = apps::load();
    let wanted: Vec<(usize, String)> = apps
        .iter()
        .enumerate()
        .filter_map(|(i, a)| a.icon.clone().map(|icon| (i, icon)))
        .collect();
    let start_icons = Box::new(move |size: u32| {
        icons::spawn(wanted, size, move |i, image| {
            send.send(Message::Icon(i, image)).is_ok()
        });
    });
    let launcher = ui::Launcher::new(
        apps,
        piece::skin("patina-launcher"),
        args.search,
        start_icons,
    );

    // At the bar's button, wherever it is; else on the bar's edge, at the
    // left, clear of the bar's reserved strip.
    let (anchor, margin, exclusive_zone) = match args.position {
        Position::Bar => piece::opening(SIZE, GAP, Anchor::LEFT),
        // No anchor: the compositor centres the surface.
        Position::Center => (Anchor::empty(), (0, 0, 0, 0), 0),
    };
    let options = Options {
        namespace: "patina-launcher".into(),
        layer: Layer::Overlay,
        anchor,
        size: SIZE,
        exclusive_zone,
        margin,
        keyboard: KeyboardInteractivity::Exclusive,
        per_output: false,
        toplevels: false,
        click_through: false,
        lock: false,
        // On GNOME, which has no layer shell: a centred window, as wofi.
        window_fallback: true,
    };
    let (host, event_loop) = Host::connect(options, launcher)?;
    event_loop
        .handle()
        .insert_source(receive, |event, _, host| match event {
            Event::Msg(Message::Icon(i, image)) => {
                host.shell.pending.push((i, image));
                host.redraw_all();
            }
            Event::Msg(Message::Close) => host.shell.close(),
            Event::Closed => {}
        })
        .map_err(|e| e.to_string())?;
    host.run(event_loop)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Args, String> {
        parse_args(args.iter().map(|a| a.to_string()))
    }

    #[test]
    fn defaults_to_the_bar_with_search_at_the_bottom() {
        assert_eq!(parse(&[]), Ok(Args::default()));
        assert_eq!(Args::default().position, Position::Bar);
        assert_eq!(Args::default().search, ui::Search::Bottom);
    }

    #[test]
    fn reads_position_and_search() {
        let args = parse(&["--position", "center", "--search=top"]).unwrap();
        assert_eq!(args.position, Position::Center);
        assert_eq!(args.search, ui::Search::Top);
    }

    #[test]
    fn rejects_what_it_doesnt_know() {
        assert!(parse(&["--position"]).is_err());
        assert!(parse(&["--position", "left"]).is_err());
        assert!(parse(&["--search", "middle"]).is_err());
        assert!(parse(&["--bogus"]).is_err());
    }
}
