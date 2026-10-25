//! The Print key's choices (ADR-0091): patina-screenshot shows them, prints
//! the one chosen and exits; a thread reads it and, once the program has
//! exited, hands it to the event loop, which takes the screen, the window
//! that was focused when the key was pressed, or starts a selection.

use std::io::Read;
use std::process::Stdio;

use smithay::reexports::calloop::channel;

use crate::state::State;

/// The chooser's layer-shell namespace, left out of every screenshot.
pub const NAMESPACE: &str = "patina-screenshot";

/// What was chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Screen,
    Window,
    Selection,
}

/// What patina-screenshot printed: a choice, or `None` for nothing (closed).
pub fn parse(out: &str) -> Option<Choice> {
    match out.lines().next()?.trim() {
        "screen" => Some(Choice::Screen),
        "window" => Some(Choice::Window),
        "selection" => Some(Choice::Selection),
        _ => None,
    }
}

impl State {
    /// Shows the choices, unless they're up already. The focused window is
    /// kept now, since the chooser takes the keyboard from it.
    pub fn open_screenshot_menu(&mut self) {
        if self.shots.menu_open || self.shots.selecting.is_some() {
            return;
        }
        let window = self.focused_window();
        let command = if window.is_some() {
            "patina-screenshot"
        } else {
            "patina-screenshot --no-window"
        };
        let sender = match &self.shots.chosen {
            Some(sender) => sender.clone(),
            None => {
                let (sender, chosen) = channel::channel::<Option<Choice>>();
                let inserted = self.loop_handle.insert_source(chosen, |event, _, state| {
                    if let channel::Event::Msg(choice) = event {
                        state.screenshot_chosen(choice);
                    }
                });
                if let Err(e) = inserted {
                    eprintln!("patina-compositor: screenshot menu: {e}");
                    return;
                }
                self.shots.chosen = Some(sender.clone());
                sender
            }
        };
        let mut child = match crate::clients::command(&self.socket_name, command)
            .stdout(Stdio::piped())
            .spawn()
        {
            Ok(child) => child,
            Err(e) => {
                eprintln!("patina-compositor: can't run patina-screenshot: {e}");
                return;
            }
        };
        self.shots.menu_open = true;
        self.shots.window = window;
        let spawned = std::thread::Builder::new()
            .name("screenshot menu".into())
            .spawn(move || {
                let mut out = String::new();
                if let Some(mut stdout) = child.stdout.take() {
                    let _ = stdout.read_to_string(&mut out);
                }
                // Gone, its surface with it, before anything is taken.
                let _ = child.wait();
                let _ = sender.send(parse(&out));
            });
        if let Err(e) = spawned {
            eprintln!("patina-compositor: screenshot menu: {e}");
            self.shots.menu_open = false;
        }
    }

    fn screenshot_chosen(&mut self, choice: Option<Choice>) {
        self.shots.menu_open = false;
        let window = self.shots.window.take();
        if self.locked() {
            return;
        }
        match choice {
            Some(Choice::Screen) => self.take_screenshot(None, None),
            Some(Choice::Window) => {
                // Unless it closed while the choices were up.
                match window.filter(|w| self.space.elements().any(|e| e == w)) {
                    Some(window) => self.take_screenshot(Some(window), None),
                    None => eprintln!("patina-compositor: no window to take a screenshot of"),
                }
            }
            Some(Choice::Selection) => self.start_selecting(),
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_choice_is_its_first_line() {
        assert_eq!(parse("screen\n"), Some(Choice::Screen));
        assert_eq!(parse("window\n"), Some(Choice::Window));
        assert_eq!(parse("selection\nmore\n"), Some(Choice::Selection));
        assert_eq!(parse(""), None);
        assert_eq!(parse("pizza\n"), None);
    }
}
