//! patina-menu: the desktop's menu, which the compositor opens at the
//! pointer on a right click on the desktop (ADR-0029), holding the style
//! (default, light or dark); and a menu of any items (ADR-0056), for pieces
//! too small to hold their own, as the bar's window tabs are.

mod items;
mod overlay;
mod tooltip;
mod ui;

use std::process::ExitCode;

use patina_shell::{Anchor, Host, KeyboardInteractivity, Layer, Options, Shell};

use crate::items::{Item, ItemMenu};
use crate::overlay::{Edge, Place};
use crate::tooltip::Tooltip;
use crate::ui::DesktopMenu;

const USAGE: &str = "usage: patina-menu [--at X,Y] [--edge top|bottom|left|right]
                   [--item ID=LABEL | --separator ...]
       patina-menu --tooltip TEXT [--at X,Y] [--edge top|bottom|left|right]

Without items, opens the desktop's menu at X,Y, in the screen's logical
pixels (the top-left corner without it), as patina-compositor does on a
right click on the desktop. Choosing a style (default, light or dark) sets GNOME's
color-scheme, which apps that follow it change with.

With items, opens a menu of them, in that order, and prints the ID of the
one chosen; nothing if it's closed without a choice. --edge bottom counts Y
up from the bottom of the screen and opens the menu upward from there, as
from a bar at the bottom. --edge left or right counts X from that edge and
opens the menu out to the side, as from a column of windows there.

With --tooltip, shows TEXT centred on X past the edge, taking no clicks or
keys, until its standard input closes.";

fn main() -> ExitCode {
    let mut place = Place::default();
    let mut items = Vec::new();
    let mut tooltip = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--at" => match args.next().as_deref().and_then(position) {
                Some(p) => place.at = p,
                None => return usage_error(),
            },
            "--edge" => match args.next().as_deref().and_then(Edge::from_name) {
                Some(edge) => place.edge = edge,
                None => return usage_error(),
            },
            "--item" => match args.next().as_deref().and_then(items::choice) {
                Some(item) => items.push(item),
                None => return usage_error(),
            },
            "--separator" => items.push(Item::Separator),
            "--tooltip" => match args.next() {
                Some(text) => tooltip = Some(text),
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
    let result = match tooltip {
        Some(text) => run(Tooltip::new(place, text), false),
        None if items.is_empty() => run(DesktopMenu::new(place.at), true),
        None => run(ItemMenu::new(place, items), true),
    };
    match result {
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

/// The skin the bar wears, as the pieces it opens do.
pub fn bar_skin() -> patina::skin::Skin {
    use patina::skin;
    let dirs: Vec<_> = skin::app_dir("patina-hud").into_iter().collect();
    let (mut skins, _) = skin::load_all(skin::BUILT_IN, &dirs);
    let name = patina_shell::hud::skin_or_default();
    skins.swap_remove(skins.iter().position(|s| s.name == name).unwrap_or(0))
}

/// Runs `menu` over the whole screen; one that's `interactive` takes the
/// keyboard and the pointer, a tooltip neither.
fn run(menu: impl Shell, interactive: bool) -> Result<(), String> {
    // The whole screen, transparent but for the menu: see ui.rs.
    let options = Options {
        namespace: "patina-menu".into(),
        layer: Layer::Overlay,
        anchor: Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
        size: (0, 0),
        exclusive_zone: -1,
        margin: (0, 0, 0, 0),
        keyboard: if interactive {
            KeyboardInteractivity::Exclusive
        } else {
            KeyboardInteractivity::None
        },
        per_output: false,
        toplevels: false,
        click_through: !interactive,
        lock: false,
        window_fallback: false,
    };
    let (host, event_loop) = Host::connect(options, menu)?;
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
