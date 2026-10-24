//! Caps Lock as a layer (ADR-0086): with `[keyboard] caps_hold = "layer"`,
//! Caps Lock never reaches the seat. Held, the keys pressed with it are
//! sent as `[caps_layer]` says ("H" = "Left"), through the remap path
//! (`remap.rs`), with the modifiers also down, so Shift+Caps+L selects to
//! the right. Tapped alone, it does `caps_tap`: Caps Lock, Escape or
//! nothing, sent when it comes up.

use smithay::backend::input::KeyState;
use smithay::input::keyboard::{FilterResult, Keycode, keysyms};
use smithay::utils::SERIAL_COUNTER;

use crate::config::{CapsHold, CapsTap, Combo};
use crate::state::State;

/// Caps Lock's evdev keycode.
pub const KEY: u32 = 58;

/// Held longer than this alone, Caps Lock isn't a tap.
pub const TAP_MS: u32 = 200;

#[derive(Default)]
pub struct Caps {
    /// When Caps Lock went down, if it's held as the layer, and whether
    /// another key was pressed with it.
    held: Option<(u32, bool)>,
}

impl Caps {
    /// Whether Caps Lock is held as the layer.
    pub fn held(&self) -> bool {
        self.held.is_some()
    }

    /// A key went down with Caps Lock held, so letting go isn't a tap.
    pub fn used(&mut self) {
        if let Some((_, used)) = self.held.as_mut() {
            *used = true;
        }
    }

    /// Caps Lock came up at `time`: whether that was a tap.
    fn release(&mut self, time: u32) -> bool {
        self.held
            .take()
            .is_some_and(|(at, used)| !used && time.wrapping_sub(at) < TAP_MS)
    }
}

impl State {
    /// Whether Caps Lock, going down or up, is the layer's: off while locked
    /// and under a fullscreen game, as remaps are, but a release always
    /// goes where its press went.
    pub fn caps_is_layer(&self, pressed: bool) -> bool {
        if !pressed {
            return self.caps.held();
        }
        self.config.keyboard.caps_hold == CapsHold::Layer
            && !self.locked()
            && self.fullscreen_on_top().is_none()
    }

    /// Caps Lock as the layer, pressed or released at `time`.
    pub fn caps_key(&mut self, pressed: bool, time: u32) {
        if pressed {
            self.caps.held = Some((time, false));
            self.super_tap = false;
            return;
        }
        if !self.caps.release(time) {
            return;
        }
        let keycode = Keycode::new(KEY + 8);
        match self.config.keyboard.caps_tap {
            CapsTap::Escape => {
                let to = with_modifiers(self, keysyms::KEY_Escape);
                self.send_remapped(keycode, to, time);
                self.release_remapped(keycode, time);
            }
            CapsTap::CapsLock => {
                let Some(keyboard) = self.seat.get_keyboard() else {
                    return;
                };
                for state in [KeyState::Pressed, KeyState::Released] {
                    keyboard.input::<(), _>(
                        self,
                        keycode,
                        state,
                        SERIAL_COUNTER.next_serial(),
                        time,
                        |_, _, _| FilterResult::Forward,
                    );
                }
            }
            CapsTap::None => {}
        }
    }
}

/// `sym` with the modifiers down now.
fn with_modifiers(state: &State, sym: u32) -> Combo {
    let real = state
        .seat
        .get_keyboard()
        .map(|k| k.modifier_state())
        .unwrap_or_default();
    held_with(
        Combo {
            logo: false,
            alt: false,
            ctrl: false,
            shift: false,
            sym,
        },
        &real,
    )
}

/// `to` with the modifiers in `real` also down.
pub fn held_with(to: Combo, real: &smithay::input::keyboard::ModifiersState) -> Combo {
    Combo {
        logo: to.logo || real.logo,
        alt: to.alt || real.alt,
        ctrl: to.ctrl || real.ctrl,
        shift: to.shift || real.shift,
        ..to
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quick_release_alone_is_a_tap() {
        let mut caps = Caps {
            held: Some((1000, false)),
        };
        assert!(caps.release(1100));
        assert!(!caps.held(), "and it's up");
    }

    #[test]
    fn a_long_hold_or_another_key_is_not_a_tap() {
        let mut caps = Caps {
            held: Some((1000, false)),
        };
        assert!(!caps.release(1000 + TAP_MS));
        let mut caps = Caps {
            held: Some((1000, false)),
        };
        caps.used();
        assert!(!caps.release(1050));
        assert!(!Caps::default().release(5), "not held, no tap");
    }

    #[test]
    fn layer_keys_keep_the_modifiers_held() {
        let to = crate::config::combo("Left").unwrap();
        let shift = smithay::input::keyboard::ModifiersState {
            shift: true,
            ..Default::default()
        };
        let sent = held_with(to, &shift);
        assert!(sent.shift && !sent.ctrl);
        assert_eq!(sent.sym, keysyms::KEY_Left);
    }
}
