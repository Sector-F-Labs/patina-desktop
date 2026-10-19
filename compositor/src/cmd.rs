//! Super as Cmd (ADR-0067): a Super key the compositor doesn't keep goes to
//! the focused window as Ctrl+key, or as Ctrl+Shift+key to a terminal, so
//! Super+T, W, C and V are the same keys in every app, as Cmd is on a Mac.
//! Only that client is told Ctrl is down; the seat's own state, and so the
//! compositor's keys, still see Super.

use std::collections::HashMap;

use smithay::input::keyboard::{KeyboardTarget, Keycode, KeysymHandle, ModifiersState};
use smithay::utils::SERIAL_COUNTER;

use crate::state::State;

#[derive(Default)]
pub struct Cmd {
    /// Keys sent on as Ctrl and still down, whose release goes the same way.
    held: Vec<Keycode>,
    /// Whether each app id seen is a terminal, from its desktop entry.
    terminals: HashMap<String, bool>,
}

/// What a client is told is down for Super with `real`: Ctrl in place of
/// Super, with Shift in a terminal; and in a terminal, Super+Shift as
/// Ctrl+Alt+Shift, so it isn't the same keys as Super alone (Super+D and
/// Super+Shift+D split two ways in Teleprinter, as Cmd+D and Cmd+Shift+D in
/// iTerm). `None` without Super, or with Ctrl or Alt as well, which make
/// other keys (Ctrl+Super+Q locks).
pub fn as_ctrl(real: &ModifiersState, terminal: bool) -> Option<ModifiersState> {
    if !real.logo || real.ctrl || real.alt {
        return None;
    }
    Some(ModifiersState {
        logo: false,
        ctrl: true,
        shift: real.shift || terminal,
        alt: terminal && real.shift,
        ..*real
    })
}

impl State {
    /// Sends `key` (keycode `code`), pressed with Super, to the focused
    /// window as Ctrl+key (ADR-0067). Returns false, sending nothing, if it
    /// isn't one: no Super, a modifier key, or no window.
    pub fn send_as_ctrl(
        &mut self,
        key: KeysymHandle<'_>,
        code: Keycode,
        real: &ModifiersState,
        time: u32,
    ) -> bool {
        if key.modified_sym().is_modifier_key() {
            return false;
        }
        let Some(keyboard) = self.seat.get_keyboard() else {
            return false;
        };
        let Some(focus) = keyboard.current_focus() else {
            return false;
        };
        let terminal = self.focused_is_terminal();
        let Some(mut mods) = as_ctrl(real, terminal) else {
            return false;
        };
        {
            let xkb = key.xkb().lock().unwrap();
            // SAFETY: the state is only read here, not kept.
            mods.serialized = mods.serialize_back(unsafe { xkb.state() });
        }
        self.cmd.held.push(code);
        let seat = self.seat.clone();
        focus.modifiers(&seat, self, mods, SERIAL_COUNTER.next_serial());
        focus.key(
            &seat,
            self,
            key,
            smithay::backend::input::KeyState::Pressed,
            SERIAL_COUNTER.next_serial(),
            time,
        );
        true
    }

    /// Releases `key` the way [`State::send_as_ctrl`] pressed it, then tells
    /// the client the modifiers really down. False if it wasn't sent so.
    pub fn release_as_ctrl(
        &mut self,
        key: KeysymHandle<'_>,
        code: Keycode,
        real: &ModifiersState,
        time: u32,
    ) -> bool {
        let Some(i) = self.cmd.held.iter().position(|&k| k == code) else {
            return false;
        };
        self.cmd.held.remove(i);
        let Some(focus) = self.seat.get_keyboard().and_then(|k| k.current_focus()) else {
            return true;
        };
        let seat = self.seat.clone();
        focus.key(
            &seat,
            self,
            key,
            smithay::backend::input::KeyState::Released,
            SERIAL_COUNTER.next_serial(),
            time,
        );
        focus.modifiers(&seat, self, *real, SERIAL_COUNTER.next_serial());
        true
    }

    /// Whether the focused window is a terminal, by its app id's desktop
    /// entry; read once per app id.
    fn focused_is_terminal(&mut self) -> bool {
        let Some(window) = self.focused_window() else {
            return false;
        };
        let app_id = match (window.x11_surface(), window.toplevel()) {
            (Some(x11), _) => x11.class(),
            (None, Some(toplevel)) => crate::toplevels::describe(toplevel.wl_surface()).1,
            (None, None) => return false,
        };
        *self
            .cmd
            .terminals
            .entry(app_id)
            .or_insert_with_key(|app_id| {
                let entries = patina_system::apps::entries();
                patina_system::apps::entry_for(&entries, app_id)
                    .is_some_and(|e| e.terminal_emulator)
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mods(logo: bool, ctrl: bool, alt: bool, shift: bool) -> ModifiersState {
        ModifiersState {
            logo,
            ctrl,
            alt,
            shift,
            ..Default::default()
        }
    }

    #[test]
    fn super_is_ctrl_and_a_terminal_adds_shift() {
        let app = as_ctrl(&mods(true, false, false, false), false).unwrap();
        assert!(app.ctrl && !app.logo && !app.shift);
        let terminal = as_ctrl(&mods(true, false, false, false), true).unwrap();
        assert!(terminal.ctrl && !terminal.logo && terminal.shift);
        let shifted = as_ctrl(&mods(true, false, false, true), false).unwrap();
        assert!(
            shifted.ctrl && shifted.shift && !shifted.alt,
            "Super+Shift+T is Ctrl+Shift+T"
        );
        let terminal_shifted = as_ctrl(&mods(true, false, false, true), true).unwrap();
        assert!(
            terminal_shifted.ctrl && terminal_shifted.shift && terminal_shifted.alt,
            "in a terminal, Super+Shift+D isn't Super+D's Ctrl+Shift+D"
        );
        assert!(!terminal.alt);
    }

    #[test]
    fn only_super_alone_or_with_shift() {
        assert!(as_ctrl(&mods(false, false, false, false), false).is_none());
        assert!(as_ctrl(&mods(false, true, false, false), false).is_none());
        assert!(as_ctrl(&mods(true, true, false, false), false).is_none());
        assert!(as_ctrl(&mods(true, false, true, false), false).is_none());
    }
}
