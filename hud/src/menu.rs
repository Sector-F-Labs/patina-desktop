//! The window tabs' menus (ADR-0056). The bar is too thin to hold a menu,
//! so a right click on a tab opens one in patina-menu, a surface over the
//! whole screen, just past the bar's edge; the choice comes back on its
//! output.

use std::io::BufRead;
use std::process::{Command, Stdio};

/// What a tab's menu offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    NewWindow,
    /// Start a pinned app with no window open (ADR-0080).
    Open,
    Pin,
    Unpin,
    Close,
}

const ITEMS: [(Choice, &str, &str); 5] = [
    (Choice::NewWindow, "new-window", "New Window"),
    (Choice::Open, "open", "Open"),
    (Choice::Pin, "pin", "Pin to Bar"),
    (Choice::Unpin, "unpin", "Unpin from Bar"),
    (Choice::Close, "close", "Close Window"),
];

/// What a tab's menu offers: a window's, New Window, Pin or Unpin and
/// Close Window; a pinned app's with no window, Open and Unpin.
pub fn choices(window: bool, pinned: bool) -> Vec<Choice> {
    let pin = if pinned { Choice::Unpin } else { Choice::Pin };
    if window {
        vec![Choice::NewWindow, pin, Choice::Close]
    } else {
        vec![Choice::Open, Choice::Unpin]
    }
}

/// What was chosen in a tab's menu: for window `window`, or with none for
/// a pinned app's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chosen {
    pub window: Option<u64>,
    pub app_id: String,
    pub choice: Choice,
}

/// patina-menu's arguments for a tab's menu of `choices`, opening at `at`.
pub fn args(at: crate::split::Popup, choices: &[Choice]) -> Vec<String> {
    let mut args = vec![
        "--at".to_string(),
        format!("{:.0},{:.0}", at.x, at.y),
        "--edge".into(),
        at.edge.into(),
    ];
    for (_, id, label) in ITEMS.iter().filter(|(c, ..)| choices.contains(c)) {
        args.push("--item".into());
        args.push(format!("{id}={label}"));
    }
    args
}

/// The choice patina-menu printed, if any.
pub fn choice(output: &str) -> Option<Choice> {
    let line = output.trim();
    ITEMS
        .iter()
        .find(|(_, id, _)| *id == line)
        .map(|(choice, ..)| *choice)
}

/// Runs patina-menu with `args` and returns what it printed: the chosen
/// item's ID, or nothing. Blocks until the menu closes.
fn run_menu(args: &[String]) -> String {
    let child = Command::new("patina-menu")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(e) => {
            eprintln!("patina-hud: can't open patina-menu: {e}");
            return String::new();
        }
    };
    let mut line = String::new();
    if let Some(out) = child.stdout.take() {
        let _ = std::io::BufReader::new(out).read_line(&mut line);
    }
    let _ = child.wait();
    line.trim().to_string()
}

/// patina-menu's arguments for a tray item's menu (ADR-0064), opening at
/// `at`: its entries in order, a submenu's after its name ("Friends ›
/// Online"), ticks shown, and the ones that can't be chosen left out, as
/// patina-menu can't grey them.
pub fn tray_args(
    at: crate::split::Popup,
    entries: &[patina_system::sni::MenuEntry],
) -> Vec<String> {
    use patina_system::sni::MenuEntry;
    fn add(entries: &[MenuEntry], prefix: &str, args: &mut Vec<String>) {
        for entry in entries {
            match entry {
                MenuEntry::Separator => {
                    // Not twice, nor first.
                    if args.last().is_some_and(|a| a != "--separator") && args.len() > 4 {
                        args.push("--separator".into());
                    }
                }
                MenuEntry::Entry { enabled: false, .. } => {}
                MenuEntry::Entry {
                    label, children, ..
                } if !children.is_empty() => {
                    add(children, &format!("{prefix}{label} › "), args);
                }
                MenuEntry::Entry {
                    id, label, checked, ..
                } => {
                    let tick = match checked {
                        Some(true) => "✔ ",
                        _ => "",
                    };
                    args.push("--item".into());
                    args.push(format!("{id}={tick}{prefix}{label}"));
                }
            }
        }
    }
    let mut args = vec![
        "--at".to_string(),
        format!("{:.0},{:.0}", at.x, at.y),
        "--edge".into(),
        at.edge.into(),
    ];
    add(entries, "", &mut args);
    if args.last().is_some_and(|a| a == "--separator") {
        args.pop();
    }
    args
}

/// patina-menu's arguments for the keyboard layouts' menu (ADR-0068),
/// opening at `at`: each layout by its index, the one in use ticked.
pub fn layout_args(
    at: crate::split::Popup,
    status: &patina_system::keyboard::Status,
) -> Vec<String> {
    let mut args = vec![
        "--at".to_string(),
        format!("{:.0},{:.0}", at.x, at.y),
        "--edge".into(),
        at.edge.into(),
    ];
    for (i, layout) in status.layouts.iter().enumerate() {
        let tick = if i == status.current { "✔ " } else { "" };
        args.push("--item".into());
        args.push(format!("{i}={tick}{}", layout.name));
    }
    args
}

/// Opens the layouts' menu with `args` on a thread, and switches to the one
/// chosen.
pub fn open_layouts(args: Vec<String>) {
    std::thread::spawn(move || {
        if let Ok(i) = run_menu(&args).parse::<usize>()
            && let Err(e) = patina_system::keyboard::set_current(i)
        {
            eprintln!("patina-hud: can't switch keyboard layout: {e}");
        }
    });
}

/// Opens a tray item's menu with `args` on a thread, and gives the chosen
/// entry's ID to `chosen`.
pub fn open_tray(args: Vec<String>, chosen: impl FnOnce(i32) + Send + 'static) {
    std::thread::spawn(move || {
        if let Ok(id) = run_menu(&args).parse::<i32>() {
            chosen(id);
        }
    });
}

/// Opens the menu with `args` on a thread, and passes on what's chosen.
/// patina-menu is found on PATH, where the compositor puts its own folder
/// first.
pub fn open(
    args: Vec<String>,
    window: Option<u64>,
    app_id: String,
    send: impl Fn(Chosen) + Send + 'static,
) {
    std::thread::spawn(move || {
        if let Some(choice) = choice(&run_menu(&args)) {
            send(Chosen {
                window,
                app_id,
                choice,
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_menu_opens_past_the_bar_with_its_items() {
        let at = crate::split::Popup {
            edge: "bottom",
            x: 212.4,
            y: 53.0,
            far: None,
        };
        assert_eq!(
            args(at, &choices(true, false)),
            [
                "--at",
                "212,53",
                "--edge",
                "bottom",
                "--item",
                "new-window=New Window",
                "--item",
                "pin=Pin to Bar",
                "--item",
                "close=Close Window"
            ]
        );
        assert_eq!(
            args(at, &choices(true, true))[6..8],
            ["--item", "unpin=Unpin from Bar"]
        );
        assert_eq!(
            args(at, &choices(false, true))[4..],
            ["--item", "open=Open", "--item", "unpin=Unpin from Bar"],
            "a pinned app with no window"
        );
    }

    #[test]
    fn the_layouts_menu_ticks_the_one_in_use() {
        use patina_system::keyboard::{Layout, Status};
        let layout = |code: &str, name: &str| Layout {
            code: code.into(),
            variant: String::new(),
            name: name.into(),
            label: code.to_uppercase(),
        };
        let status = Status {
            layouts: vec![layout("us", "English (US)"), layout("se", "Swedish")],
            current: 1,
        };
        let at = crate::split::Popup {
            edge: "top",
            x: 10.0,
            y: 20.0,
            far: None,
        };
        assert_eq!(
            layout_args(at, &status)[4..],
            ["--item", "0=English (US)", "--item", "1=✔ Swedish"]
        );
    }

    #[test]
    fn the_printed_id_is_the_choice() {
        assert_eq!(choice("new-window\n"), Some(Choice::NewWindow));
        assert_eq!(choice("close"), Some(Choice::Close));
        assert_eq!(choice("unpin"), Some(Choice::Unpin));
        assert_eq!(choice(""), None, "closed without a choice");
        assert_eq!(choice("other"), None);
    }
}
