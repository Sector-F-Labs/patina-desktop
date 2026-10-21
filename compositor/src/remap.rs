//! Remaps (ADR-0076): a key pressed with its modifiers, from `[remap]`, is
//! sent to the focused window as another ("Alt+T" = "Ctrl+T"). The key sent
//! goes through the seat, so the window gets a real key, and only that
//! window is told the remap's modifiers are down, as for Super as Ctrl
//! (`cmd.rs`); the compositor's keys still see the ones really down. Its
//! release goes with the key's.

use smithay::backend::input::KeyState;
use smithay::input::keyboard::{FilterResult, KeyboardTarget, Keycode, Keysym, ModifiersState};
use smithay::utils::SERIAL_COUNTER;

use crate::config::Combo;
use crate::state::State;

#[derive(Default)]
pub struct Remapped {
    /// Keys pressed and remapped, still down, with the key sent for each.
    held: Vec<(Keycode, Keycode)>,
}

impl Remapped {
    /// Whether `key` was remapped when pressed, so its release is too.
    pub fn holds(&self, key: Keycode) -> bool {
        self.held.iter().any(|(k, _)| *k == key)
    }
}

impl State {
    /// Sends `to` to the focused window for `key`, pressed. Nothing if the
    /// first layout has no key for it.
    pub fn send_remapped(&mut self, key: Keycode, to: Combo, time: u32) {
        let Some(keyboard) = self.seat.get_keyboard() else {
            return;
        };
        let sent = keyboard.with_xkb_state(self, |context| {
            let xkb = context.xkb().lock().expect("xkb isn't poisoned");
            // SAFETY: the keymap is only read here, not kept.
            crate::typing::key_for(unsafe { xkb.keymap() }, 0, Keysym::new(to.sym))
        });
        let Some(sent) = sent else {
            eprintln!(
                "patina-compositor: no key for {} to remap to",
                smithay::input::keyboard::xkb::keysym_get_name(Keysym::new(to.sym))
            );
            return;
        };
        self.remapped.held.push((key, sent));
        let seat = self.seat.clone();
        keyboard.input::<(), _>(
            self,
            sent,
            KeyState::Pressed,
            SERIAL_COUNTER.next_serial(),
            time,
            |state, real, handle| {
                let Some(focus) = state.seat.get_keyboard().and_then(|k| k.current_focus()) else {
                    return FilterResult::Intercept(());
                };
                let mut mods = ModifiersState {
                    logo: to.logo,
                    ctrl: to.ctrl,
                    alt: to.alt,
                    shift: to.shift,
                    ..*real
                };
                {
                    let xkb = handle.xkb().lock().unwrap();
                    // SAFETY: the state is only read here, not kept.
                    mods.serialized = mods.serialize_back(unsafe { xkb.state() });
                }
                focus.modifiers(&seat, state, mods, SERIAL_COUNTER.next_serial());
                focus.key(
                    &seat,
                    state,
                    handle,
                    KeyState::Pressed,
                    SERIAL_COUNTER.next_serial(),
                    time,
                );
                FilterResult::Intercept(())
            },
        );
    }

    /// Lets go of what was sent for `key`, if it was remapped, then tells
    /// the window the modifiers really down.
    pub fn release_remapped(&mut self, key: Keycode, time: u32) {
        let Some(i) = self.remapped.held.iter().position(|(k, _)| *k == key) else {
            return;
        };
        let (_, sent) = self.remapped.held.remove(i);
        let Some(keyboard) = self.seat.get_keyboard() else {
            return;
        };
        let seat = self.seat.clone();
        keyboard.input::<(), _>(
            self,
            sent,
            KeyState::Released,
            SERIAL_COUNTER.next_serial(),
            time,
            |state, real, handle| {
                if let Some(focus) = state.seat.get_keyboard().and_then(|k| k.current_focus()) {
                    focus.key(
                        &seat,
                        state,
                        handle,
                        KeyState::Released,
                        SERIAL_COUNTER.next_serial(),
                        time,
                    );
                    focus.modifiers(&seat, state, *real, SERIAL_COUNTER.next_serial());
                }
                FilterResult::Intercept(())
            },
        );
    }
}
