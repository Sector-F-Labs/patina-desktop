//! Text typed into the focused window for another program (ADR-0071):
//! dictation's words, from `Commit` on `com.sectorflabs.Patina.Input`. And
//! the dictate key (ADR-0072): `DictationStarted` when it goes down,
//! `DictationStopped` when it comes up, for patina-dictate to record between.
//!
//! It goes through the clipboard: the clipboard as it was is read out of
//! the app that owns it, the text put there by the compositor, the focused
//! window sent the paste key (Ctrl+V, Ctrl+Shift+V in a terminal), and the
//! old clipboard given back once the window has read the text. Wayland
//! windows only; X11 ones are left alone.

pub mod dbus;

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::os::fd::OwnedFd;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use smithay::backend::input::KeyState;
use smithay::input::keyboard::{FilterResult, Keycode, Keysym, xkb};
use smithay::reexports::calloop::{
    channel,
    timer::{TimeoutAction, Timer},
};
use smithay::utils::SERIAL_COUNTER;
use smithay::wayland::selection::SelectionTarget;
use smithay::wayland::selection::data_device::{
    clear_data_device_selection, current_data_device_selection_userdata,
    request_data_device_client_selection, set_data_device_selection,
};

use crate::state::State;

/// The types text is offered as, the first the usual one.
const TEXT_TYPES: [&str; 5] = [
    "text/plain;charset=utf-8",
    "text/plain",
    "UTF8_STRING",
    "STRING",
    "TEXT",
];

/// How long the clipboard's owner has to hand over what's on it.
const SAVING: Duration = Duration::from_millis(500);
/// The most kept of one type, so a huge image can't hold up the paste.
const MOST_SAVED: u64 = 64 << 20;
/// After the window read the text, before the clipboard is given back.
const AFTER_READ: Duration = Duration::from_millis(150);
/// The clipboard is given back after this even if the text wasn't read.
const UNREAD: Duration = Duration::from_secs(1);
/// How long a paste waits for modifiers held by hand to be let go, so the
/// window sees Ctrl+V and not Ctrl+Alt+V; then it pastes anyway.
const LET_GO: Duration = Duration::from_secs(2);
/// How often it looks.
const LET_GO_STEP: Duration = Duration::from_millis(50);

/// What's on the clipboard when the compositor put it there: the
/// selection's user data.
#[derive(Debug, Clone)]
pub enum Served {
    /// Text being typed.
    Text(Arc<str>),
    /// The clipboard as it was, given back: each type with its bytes.
    Saved(Arc<Vec<(String, Vec<u8>)>>),
}

impl Served {
    fn types(&self) -> Vec<String> {
        match self {
            Served::Text(_) => TEXT_TYPES.iter().map(|t| t.to_string()).collect(),
            Served::Saved(saved) => saved.iter().map(|(t, _)| t.clone()).collect(),
        }
    }

    /// The bytes for `mime_type`, if it's one of this one's.
    fn bytes(&self, mime_type: &str) -> Option<Vec<u8>> {
        match self {
            Served::Text(text) => TEXT_TYPES
                .contains(&mime_type)
                .then(|| text.as_bytes().to_vec()),
            Served::Saved(saved) => saved
                .iter()
                .find(|(t, _)| t == mime_type)
                .map(|(_, b)| b.clone()),
        }
    }
}

#[derive(Default)]
pub struct Typing {
    service: Option<dbus::Service>,
    /// How dictation is being held, while it is.
    dictating: Option<Held>,
    /// Since when a paste has waited for modifiers to be let go.
    waiting: Option<Instant>,
    /// The types an app's clipboard offers, as it last said.
    offered: Vec<String>,
    /// Texts waiting their turn.
    queue: VecDeque<Arc<str>>,
    stage: Stage,
    /// Bumped by each paste, so a timer for an earlier one does nothing.
    paste: u64,
    /// The paste key's modifiers, held down until the window has read the
    /// text, as a hand would: some apps (Teleprinter) look at them when
    /// the paste arrives, not when V went down.
    held: Vec<Keycode>,
}

/// What holds dictation: a key with its modifiers (by the key's code), or
/// modifiers alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Held {
    Key(u32),
    Chord,
}

#[derive(Default)]
enum Stage {
    #[default]
    Idle,
    /// The clipboard's owner is handing it over.
    Saving,
    /// A text is on the clipboard and the paste key sent; the clipboard
    /// that was is given back after.
    Pasted { saved: Option<Served> },
}

impl State {
    /// Serves `com.sectorflabs.Patina.Input`.
    pub fn start_typing(&mut self) {
        match dbus::serve(&self.loop_handle) {
            Ok(service) => self.typing.service = Some(service),
            Err(e) => eprintln!("patina-compositor: typing isn't on D-Bus: {e}"),
        }
    }

    /// Types `text` into the focused window, after any still waiting.
    pub fn commit_text(&mut self, text: String) {
        if text.is_empty() {
            return;
        }
        self.typing.queue.push_back(text.into());
        if matches!(self.typing.stage, Stage::Idle) {
            self.save_clipboard();
        }
    }

    /// The dictate key went down: dictation starts, unless it has (a key
    /// repeating).
    pub fn dictate_key_pressed(&mut self, code: u32) {
        self.start_dictation(Held::Key(code));
    }

    /// The dictate chord's modifiers are all down.
    pub fn dictate_chord_pressed(&mut self) {
        self.start_dictation(Held::Chord);
    }

    fn start_dictation(&mut self, held: Held) {
        if self.typing.dictating.is_none() {
            self.typing.dictating = Some(held);
            self.announce_dictation(true);
        }
    }

    /// A key came up: dictation stops if it was the dictate key, or any key
    /// of the chord holding it.
    pub fn dictate_key_released(&mut self, code: u32) {
        let stops = match self.typing.dictating {
            Some(Held::Key(key)) => key == code,
            Some(Held::Chord) => true,
            None => false,
        };
        if stops {
            self.typing.dictating = None;
            self.announce_dictation(false);
        }
    }

    fn announce_dictation(&self, started: bool) {
        if let Some(service) = &self.typing.service {
            service.announce(started);
        }
    }

    /// An app put something on the clipboard, or emptied it.
    pub fn clipboard_offered(&mut self, types: Vec<String>) {
        self.typing.offered = types;
    }

    /// A client reads what the compositor put on the clipboard.
    pub fn send_served(&mut self, target: SelectionTarget, mime_type: &str, fd: OwnedFd) {
        let served = match target {
            SelectionTarget::Clipboard => {
                current_data_device_selection_userdata(&self.seat).map(|s| s.clone())
            }
            SelectionTarget::Primary => None,
        };
        let Some(served) = served else { return };
        if let Some(bytes) = served.bytes(mime_type) {
            // On a thread: a big clipboard fills the pipe before the reader
            // reads, and the loop mustn't wait for it.
            std::thread::spawn(move || {
                let _ = std::fs::File::from(fd).write_all(&bytes);
            });
        }
        if matches!(served, Served::Text(_)) {
            self.after_paste(AFTER_READ);
        }
    }

    /// Reads the clipboard out of the app that owns it, then pastes. One the
    /// compositor owns is kept as it is; an empty one is nothing to keep.
    fn save_clipboard(&mut self) {
        let ours = current_data_device_selection_userdata(&self.seat).map(|s| s.clone());
        if ours.is_some() || self.typing.offered.is_empty() {
            self.paste_next(ours);
            return;
        }
        let mut pipes = Vec::new();
        for mime_type in self.typing.offered.clone() {
            let Ok((reader, writer)) = std::io::pipe() else {
                continue;
            };
            if request_data_device_client_selection(&self.seat, mime_type.clone(), writer.into())
                .is_ok()
            {
                pipes.push((mime_type, reader));
            }
        }
        if pipes.is_empty() {
            self.paste_next(None);
            return;
        }
        let (send, saved) = channel::channel::<Vec<(String, Vec<u8>)>>();
        let inserted = self.loop_handle.insert_source(saved, |event, _, state| {
            if let channel::Event::Msg(saved) = event {
                let saved = (!saved.is_empty()).then(|| Served::Saved(Arc::new(saved)));
                state.paste_next(saved);
            }
        });
        if let Err(e) = inserted {
            eprintln!("patina-compositor: typing: {e}");
            self.paste_next(None);
            return;
        }
        self.typing.stage = Stage::Saving;
        std::thread::spawn(move || {
            let _ = send.send(read_all(pipes, Instant::now() + SAVING));
        });
    }

    /// Pastes the next text, or gives the clipboard back if there's none.
    /// Waits, for a while, for modifiers held by hand to be let go.
    fn paste_next(&mut self, saved: Option<Served>) {
        if !self.typing.queue.is_empty() && self.modifiers_down() {
            let since = *self.typing.waiting.get_or_insert_with(Instant::now);
            if since.elapsed() < LET_GO {
                self.typing.stage = Stage::Pasted { saved };
                self.after_paste(LET_GO_STEP);
                return;
            }
        }
        self.typing.waiting = None;
        let Some(text) = self.typing.queue.pop_front() else {
            self.give_back(saved);
            return;
        };
        self.typing.stage = Stage::Pasted { saved };
        let focus = self.focused_window();
        if focus.is_none_or(|w| w.x11_surface().is_some()) {
            eprintln!("patina-compositor: typing: no Wayland window has the focus");
            self.after_paste(Duration::ZERO);
            return;
        }
        let served = Served::Text(text);
        set_data_device_selection(&self.display_handle, &self.seat, served.types(), served);
        if !self.press_paste() {
            eprintln!("patina-compositor: typing: the keymap has no V");
        }
        self.after_paste(UNREAD);
    }

    /// In `delay`, the next text, or the clipboard given back. Replaces the
    /// paste's earlier timer.
    fn after_paste(&mut self, delay: Duration) {
        self.typing.paste += 1;
        let paste = self.typing.paste;
        let timer = Timer::from_duration(delay);
        let _ = self.loop_handle.insert_source(timer, move |_, _, state| {
            if state.typing.paste == paste
                && let Stage::Pasted { saved } = std::mem::take(&mut state.typing.stage)
            {
                state.release_held();
                state.paste_next(saved);
            }
            TimeoutAction::Drop
        });
    }

    /// Puts back the clipboard there was before the texts.
    fn give_back(&mut self, saved: Option<Served>) {
        self.typing.stage = Stage::Idle;
        match saved {
            Some(served) => {
                set_data_device_selection(&self.display_handle, &self.seat, served.types(), served)
            }
            None => clear_data_device_selection(&self.display_handle, &self.seat),
        }
    }

    /// Ctrl+V to the focused window, Ctrl+Shift+V to a terminal, past the
    /// compositor's own keys: V pressed and let go, Ctrl and Shift left
    /// down until [`State::release_held`]. False if the keymap has no V.
    fn press_paste(&mut self) -> bool {
        let Some(keyboard) = self.seat.get_keyboard() else {
            return false;
        };
        let v = keyboard.with_xkb_state(self, |context| {
            let xkb = context.xkb().lock().expect("xkb isn't poisoned");
            let layout = xkb.active_layout().0;
            // SAFETY: the keymap is only read here, not kept.
            key_for(unsafe { xkb.keymap() }, layout, Keysym::v)
        });
        let Some(v) = v else { return false };
        let mut held = vec![LEFT_CTRL];
        if self.focused_is_terminal() {
            held.push(LEFT_SHIFT);
        }
        let presses = held.iter().chain([&v]).map(|&k| (k, KeyState::Pressed));
        self.input_keys(presses.chain([(v, KeyState::Released)]));
        self.typing.held = held;
        true
    }

    /// Whether a modifier is down on the keyboard.
    fn modifiers_down(&self) -> bool {
        self.seat.get_keyboard().is_some_and(|k| {
            let m = k.modifier_state();
            m.ctrl || m.alt || m.shift || m.logo
        })
    }

    /// Lets go of the paste key's modifiers.
    fn release_held(&mut self) {
        let held = std::mem::take(&mut self.typing.held);
        self.input_keys(held.into_iter().rev().map(|k| (k, KeyState::Released)));
    }

    /// Keys into the seat as if typed, past the compositor's own.
    fn input_keys(&mut self, keys: impl IntoIterator<Item = (Keycode, KeyState)>) {
        let Some(keyboard) = self.seat.get_keyboard() else {
            return;
        };
        let time = self.start.elapsed().as_millis() as u32;
        for (key, state) in keys {
            keyboard.input::<(), _>(
                self,
                key,
                state,
                SERIAL_COUNTER.next_serial(),
                time,
                |_, _, _| FilterResult::Forward,
            );
        }
    }
}

/// evdev's codes plus xkb's 8.
const LEFT_CTRL: Keycode = Keycode::new(29 + 8);
const LEFT_SHIFT: Keycode = Keycode::new(42 + 8);

/// The key that gives `sym` unshifted in `layout`.
fn key_for(keymap: &xkb::Keymap, layout: u32, sym: Keysym) -> Option<Keycode> {
    (keymap.min_keycode().raw()..=keymap.max_keycode().raw())
        .map(Keycode::new)
        .find(|&key| keymap.key_get_syms_by_level(key, layout, 0).contains(&sym))
}

/// Each type's bytes, as many as arrive by `deadline`: an app that never
/// answers is left behind, and its type with it.
fn read_all(
    pipes: Vec<(String, std::io::PipeReader)>,
    deadline: Instant,
) -> Vec<(String, Vec<u8>)> {
    let count = pipes.len();
    let (send, read) = mpsc::channel();
    for (i, (mime_type, reader)) in pipes.into_iter().enumerate() {
        let send = send.clone();
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            if reader.take(MOST_SAVED).read_to_end(&mut bytes).is_ok() {
                let _ = send.send((i, mime_type, bytes));
            }
        });
    }
    drop(send);
    let mut saved = Vec::with_capacity(count);
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        match read.recv_timeout(left) {
            Ok(one) => saved.push(one),
            Err(_) => break,
        }
    }
    // In the order the app offered them, its preferred first.
    saved.sort_by_key(|(i, _, _)| *i);
    saved.into_iter().map(|(_, t, b)| (t, b)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_served_as_text_only() {
        let served = Served::Text("hej".into());
        assert_eq!(served.bytes("text/plain;charset=utf-8").unwrap(), b"hej");
        assert_eq!(served.bytes("UTF8_STRING").unwrap(), b"hej");
        assert!(served.bytes("image/png").is_none());
        assert_eq!(served.types()[0], "text/plain;charset=utf-8");
    }

    #[test]
    fn the_saved_clipboard_is_given_back_as_it_was() {
        let saved = Served::Saved(Arc::new(vec![
            ("image/png".into(), vec![1, 2]),
            ("text/plain".into(), b"alt".to_vec()),
        ]));
        assert_eq!(saved.types(), ["image/png", "text/plain"]);
        assert_eq!(saved.bytes("image/png").unwrap(), [1, 2]);
        assert!(saved.bytes("text/html").is_none());
    }

    #[test]
    fn v_is_found_wherever_the_layout_puts_it() {
        let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        let keymap = |layout: &str, variant: &str| {
            xkb::Keymap::new_from_names(&context, "", "", layout, variant, None, 0).unwrap()
        };
        // evdev 47 on QWERTY; on Dvorak, V is where QWERTY has the full stop.
        assert_eq!(key_for(&keymap("us", ""), 0, Keysym::v), Some(Keycode::new(55)));
        assert_eq!(key_for(&keymap("us", "dvorak"), 0, Keysym::v), Some(Keycode::new(60)));
    }

    #[test]
    fn reading_stops_at_the_deadline() {
        let (quick, mut quick_writer) = std::io::pipe().unwrap();
        let (_slow, slow_writer) = std::io::pipe().unwrap();
        quick_writer.write_all(b"text").unwrap();
        drop(quick_writer);
        let started = Instant::now();
        let saved = read_all(
            vec![("text/plain".into(), quick), ("image/png".into(), _slow)],
            Instant::now() + Duration::from_millis(100),
        );
        assert_eq!(saved, [("text/plain".to_string(), b"text".to_vec())]);
        assert!(started.elapsed() < Duration::from_secs(1));
        drop(slow_writer);
    }
}
