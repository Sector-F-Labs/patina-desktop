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

/// How far past the bar's edge the menu opens.
const SPACE: f32 = 4.0;

/// patina-menu's arguments for a tab's menu: X from the screen's left, and
/// `from_edge`, how far the bar reaches in from its edge (`top` or
/// `bottom`).
pub fn args(x: f32, from_edge: f32, edge: &str) -> Vec<String> {
    let mut args = vec![
        "--at".to_string(),
        format!("{:.0},{:.0}", x, from_edge + SPACE),
        "--edge".into(),
        edge.into(),
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
        let child = Command::new("patina-menu")
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .spawn();
        let mut child = match child {
            Ok(child) => child,
            Err(e) => {
                eprintln!("patina-bar: can't open patina-menu: {e}");
                return;
            }
        };
        let mut line = String::new();
        if let Some(out) = child.stdout.take() {
            let _ = std::io::BufReader::new(out).read_line(&mut line);
        }
        let _ = child.wait();
        if let Some(choice) = choice(&line) {
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
            args(212.4, 49.0, "bottom"),
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
    fn the_printed_id_is_the_choice() {
        assert_eq!(choice("new-window\n"), Some(Choice::NewWindow));
        assert_eq!(choice("close"), Some(Choice::Close));
        assert_eq!(choice(""), None, "closed without a choice");
        assert_eq!(choice("other"), None);
    }
}
