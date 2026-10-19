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
    Close,
}

const ITEMS: [(Choice, &str, &str); 2] = [
    (Choice::NewWindow, "new-window", "New Window"),
    (Choice::Close, "close", "Close Window"),
];

/// What was chosen in window `window`'s menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chosen {
    pub window: u64,
    pub app_id: String,
    pub choice: Choice,
}

/// patina-menu's arguments for a tab's menu, opening at `at`.
pub fn args(at: crate::split::Popup) -> Vec<String> {
    let mut args = vec![
        "--at".to_string(),
        format!("{:.0},{:.0}", at.x, at.y),
        "--edge".into(),
        at.edge.into(),
    ];
    for (_, id, label) in ITEMS {
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
            eprintln!("patina-bar: can't open patina-menu: {e}");
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
            eprintln!("patina-bar: can't switch keyboard layout: {e}");
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
    window: u64,
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
        assert_eq!(
            args(crate::split::Popup {
                edge: "bottom",
                x: 212.4,
                y: 53.0,
                far: None
            }),
            [
                "--at",
                "212,53",
                "--edge",
                "bottom",
                "--item",
                "new-window=New Window",
                "--item",
                "close=Close Window"
            ]
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
        assert_eq!(choice(""), None, "closed without a choice");
        assert_eq!(choice("other"), None);
    }
}
