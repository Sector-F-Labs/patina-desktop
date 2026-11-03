//! The Print key's choices (ADR-0091, ADR-0092): patina-screenshot shows
//! them, prints the one chosen and exits; a thread reads it and, once the
//! program has exited, hands it to the event loop, which takes the screen,
//! the window that was focused when the key was pressed, or starts a
//! selection; or starts recording the screen or a selection. While a
//! recording goes, `patina-screenshot --recording` shows it, and prints
//! `stop` when it's stopped there.

use std::io::Read;
use std::process::{ChildStdin, Stdio};

use smithay::reexports::calloop::channel;

use crate::state::State;

/// The chooser's layer-shell namespace, left out of every screenshot and
/// recording (the recording's indicator has it too).
pub const NAMESPACE: &str = "patina-screenshot";

/// What was chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Screen,
    Window,
    Selection,
    RecordScreen,
    RecordSelection,
}

/// What reaches the event loop from patina-screenshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// The choices closed, with this chosen or nothing.
    Chosen(Option<Choice>),
    /// Stop was pressed on the recording's indicator.
    Stop,
}

/// What patina-screenshot printed: a choice, or `None` for nothing (closed).
pub fn parse(out: &str) -> Option<Choice> {
    match out.lines().next()?.trim() {
        "screen" => Some(Choice::Screen),
        "window" => Some(Choice::Window),
        "selection" => Some(Choice::Selection),
        "record-screen" => Some(Choice::RecordScreen),
        "record-selection" => Some(Choice::RecordSelection),
        _ => None,
    }
}

impl State {
    /// Where patina-screenshot's threads send what it printed, set up on
    /// first use.
    fn screenshot_events(&mut self) -> Option<channel::Sender<Event>> {
        if let Some(sender) = &self.shots.events {
            return Some(sender.clone());
        }
        let (sender, events) = channel::channel::<Event>();
        let inserted = self
            .loop_handle
            .insert_source(events, |event, _, state| match event {
                channel::Event::Msg(Event::Chosen(choice)) => state.screenshot_chosen(choice),
                channel::Event::Msg(Event::Stop) => state.stop_recording(),
                channel::Event::Closed => {}
            });
        if let Err(e) = inserted {
            eprintln!("patina-compositor: screenshot menu: {e}");
            return None;
        }
        self.shots.events = Some(sender.clone());
        Some(sender)
    }

    /// Shows the choices, unless they're up already; while recording, the
    /// key stops it instead. The focused window is kept now, since the
    /// chooser takes the keyboard from it.
    pub fn open_screenshot_menu(&mut self) {
        if self.recording() {
            self.stop_recording();
            return;
        }
        if self.shots.menu_open || self.shots.selecting.is_some() {
            return;
        }
        // Something has the whole screen, a game most likely: the chooser
        // would take the keyboard from it, so the key takes the screen.
        if self.fullscreen_on_top().is_some() {
            self.screenshot(crate::screenshot::Shot::Desktop);
            return;
        }
        let window = self.focused_window();
        let command = if window.is_some() {
            "patina-screenshot"
        } else {
            "patina-screenshot --no-window"
        };
        let Some(sender) = self.screenshot_events() else {
            return;
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
                let _ = sender.send(Event::Chosen(parse(&out)));
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
        use crate::screenshot::select::Purpose;
        match choice {
            Some(Choice::Screen) => self.take_screenshot(None, None),
            Some(Choice::Window) => {
                // Unless it closed while the choices were up.
                match window.filter(|w| self.space.elements().any(|e| e == w)) {
                    Some(window) => self.take_screenshot(Some(window), None),
                    None => eprintln!("patina-compositor: no window to take a screenshot of"),
                }
            }
            Some(Choice::Selection) => self.start_selecting(Purpose::Screenshot),
            Some(Choice::RecordScreen) => self.start_recording(None),
            Some(Choice::RecordSelection) => self.start_selecting(Purpose::Recording),
            None => {}
        }
    }

    /// Starts the indicator that shows a recording is going; it goes when
    /// what's returned is dropped, and Stop on it stops the recording.
    pub fn show_recording_indicator(&mut self) -> Option<ChildStdin> {
        let sender = self.screenshot_events()?;
        let mut child =
            match crate::clients::command(&self.socket_name, "patina-screenshot --recording")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
            {
                Ok(child) => child,
                Err(e) => {
                    eprintln!("patina-compositor: can't run patina-screenshot: {e}");
                    return None;
                }
            };
        let stdin = child.stdin.take();
        let spawned = std::thread::Builder::new()
            .name("recording indicator".into())
            .spawn(move || {
                let mut out = String::new();
                if let Some(mut stdout) = child.stdout.take() {
                    let _ = stdout.read_to_string(&mut out);
                }
                let _ = child.wait();
                if out.trim() == "stop" {
                    let _ = sender.send(Event::Stop);
                }
            });
        if let Err(e) = spawned {
            eprintln!("patina-compositor: recording indicator: {e}");
        }
        stdin
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
        assert_eq!(parse("record-screen\n"), Some(Choice::RecordScreen));
        assert_eq!(parse("record-selection\n"), Some(Choice::RecordSelection));
        assert_eq!(parse(""), None);
        assert_eq!(parse("pizza\n"), None);
    }
}
