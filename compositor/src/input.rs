//! Input from Smithay's input backends (winit when nested, libinput on the
//! hardware) into the compositor's seat, and the keys the compositor keeps
//! for itself.

use smithay::{
    backend::input::{
        AbsolutePositionEvent, Axis, AxisSource, ButtonState, Event, InputBackend, InputEvent,
        KeyState, KeyboardKeyEvent, PointerAxisEvent, PointerButtonEvent, PointerMotionEvent,
    },
    input::{
        keyboard::{FilterResult, ModifiersState, keysyms},
        pointer::AxisFrame,
    },
    utils::{Logical, Point, SERIAL_COUNTER},
};

use crate::state::State;

/// What a key the compositor keeps asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyAction {
    /// Switch to text console `n` (Ctrl+Alt+F`n`), on the hardware.
    SwitchVt(i32),
    /// Quit the compositor: Ctrl+Alt+Backspace, the way out of a session
    /// that's gone wrong.
    Quit,
    /// The next keyboard layout (Super+Space, as on GNOME).
    NextLayout,
    /// Read config.toml again (Super+Shift+R).
    Reload,
    /// A brightness or volume key.
    Media(crate::media::MediaKey),
}

/// The compositor's own keys; everything else goes to the focused client.
pub fn key_action(modifiers: &ModifiersState, sym: u32) -> Option<KeyAction> {
    if let Some(key) = crate::media::media_key(sym) {
        return Some(KeyAction::Media(key));
    }
    if (keysyms::KEY_XF86Switch_VT_1..=keysyms::KEY_XF86Switch_VT_12).contains(&sym) {
        return Some(KeyAction::SwitchVt(
            (sym - keysyms::KEY_XF86Switch_VT_1 + 1) as i32,
        ));
    }
    if modifiers.ctrl && modifiers.alt && sym == keysyms::KEY_BackSpace {
        return Some(KeyAction::Quit);
    }
    if modifiers.logo && sym == keysyms::KEY_space {
        return Some(KeyAction::NextLayout);
    }
    // With Shift held, the keysym is the capital.
    if modifiers.logo && modifiers.shift && (sym == keysyms::KEY_R || sym == keysyms::KEY_r) {
        return Some(KeyAction::Reload);
    }
    None
}

impl State {
    /// `code` is an evdev keycode. Returns the compositor's own action if
    /// the key was one of its keys, and doesn't pass it on.
    pub fn key(&mut self, code: u32, pressed: bool, time: u32) -> Option<KeyAction> {
        let keyboard = self.seat.get_keyboard().expect("seat has a keyboard");
        let state = if pressed {
            KeyState::Pressed
        } else {
            KeyState::Released
        };
        let mut sym = 0;
        let action = keyboard.input::<KeyAction, _>(
            self,
            smithay::input::keyboard::Keycode::new(code + 8),
            state,
            SERIAL_COUNTER.next_serial(),
            time,
            |_, modifiers, handle| {
                sym = handle.modified_sym().raw();
                match (pressed, key_action(modifiers, sym)) {
                    (true, Some(action)) => FilterResult::Intercept(action),
                    _ => FilterResult::Forward,
                }
            },
        );
        // Super tapped on its own (pressed and released with nothing in
        // between) opens the launcher; any other key or a click cancels it.
        let is_super = sym == keysyms::KEY_Super_L || sym == keysyms::KEY_Super_R;
        if pressed {
            self.super_tap = is_super;
        } else if is_super
            && std::mem::take(&mut self.super_tap)
            && !self.config.launcher.is_empty()
        {
            self.spawn_client(&self.config.launcher.clone());
        }
        match action {
            Some(KeyAction::NextLayout) => {
                keyboard.with_xkb_state(self, |mut xkb| xkb.cycle_next_layout());
                None
            }
            Some(KeyAction::Reload) => {
                self.reload_config();
                None
            }
            Some(KeyAction::Media(key)) => {
                crate::media::press(key, &self.config.media, self.socket_name.clone());
                None
            }
            other => other,
        }
    }

    /// The output's area, for absolute positions and keeping the pointer on
    /// screen.
    fn screen(&self) -> smithay::utils::Rectangle<i32, Logical> {
        self.space.output_geometry(&self.output).unwrap_or_default()
    }
}

/// Feeds one event into the seat. Returns a key action for the backend to
/// carry out (switching consoles is the session's job).
pub fn handle<B: InputBackend>(state: &mut State, event: InputEvent<B>) -> Option<KeyAction> {
    match event {
        InputEvent::PointerMotion { event } => {
            let screen = state.screen();
            let current = state
                .seat
                .get_pointer()
                .map(|p| p.current_location())
                .unwrap_or_default();
            let moved = current + event.delta();
            let clamped = Point::from((
                moved.x.clamp(
                    screen.loc.x as f64,
                    (screen.loc.x + screen.size.w - 1) as f64,
                ),
                moved.y.clamp(
                    screen.loc.y as f64,
                    (screen.loc.y + screen.size.h - 1) as f64,
                ),
            ));
            state.pointer_motion(clamped, event.time_msec());
        }
        InputEvent::PointerMotionAbsolute { event } => {
            let screen = state.screen();
            let pos = event.position_transformed(screen.size) + screen.loc.to_f64();
            state.pointer_motion(pos, event.time_msec());
        }
        InputEvent::PointerButton { event } => state.pointer_button(
            event.button_code(),
            event.state() == ButtonState::Pressed,
            event.time_msec(),
        ),
        InputEvent::PointerAxis { event } => {
            let mut frame = AxisFrame::new(event.time_msec()).source(event.source());
            for axis in [Axis::Horizontal, Axis::Vertical] {
                let v120 = event.amount_v120(axis);
                let amount = event
                    .amount(axis)
                    .unwrap_or_else(|| v120.unwrap_or(0.0) * 15.0 / 120.0);
                if amount != 0.0 {
                    frame = frame.value(axis, amount);
                    if let Some(v120) = v120 {
                        frame = frame.v120(axis, v120 as i32);
                    }
                }
                if event.source() == AxisSource::Finger && event.amount(axis) == Some(0.0) {
                    frame = frame.stop(axis);
                }
            }
            state.pointer_axis(frame);
        }
        InputEvent::Keyboard { event } => {
            // xkb keycodes are evdev's plus 8.
            let code = event.key_code().raw().saturating_sub(8);
            return state.key(code, event.state() == KeyState::Pressed, event.time_msec());
        }
        _ => {}
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mods(ctrl: bool, alt: bool) -> ModifiersState {
        ModifiersState {
            ctrl,
            alt,
            ..Default::default()
        }
    }

    #[test]
    fn console_keys_switch_consoles() {
        assert_eq!(
            key_action(&mods(true, true), keysyms::KEY_XF86Switch_VT_3),
            Some(KeyAction::SwitchVt(3))
        );
        assert_eq!(
            key_action(&mods(true, true), keysyms::KEY_XF86Switch_VT_12),
            Some(KeyAction::SwitchVt(12))
        );
    }

    #[test]
    fn super_space_switches_layout() {
        let logo = ModifiersState {
            logo: true,
            ..Default::default()
        };
        assert_eq!(
            key_action(&logo, keysyms::KEY_space),
            Some(KeyAction::NextLayout)
        );
        assert_eq!(key_action(&mods(false, false), keysyms::KEY_space), None);
    }

    #[test]
    fn super_shift_r_reloads() {
        let keys = ModifiersState {
            logo: true,
            shift: true,
            ..Default::default()
        };
        assert_eq!(key_action(&keys, keysyms::KEY_R), Some(KeyAction::Reload));
        let no_shift = ModifiersState {
            logo: true,
            ..Default::default()
        };
        assert_eq!(key_action(&no_shift, keysyms::KEY_r), None);
    }

    #[test]
    fn ctrl_alt_backspace_quits_and_backspace_alone_types() {
        assert_eq!(
            key_action(&mods(true, true), keysyms::KEY_BackSpace),
            Some(KeyAction::Quit)
        );
        assert_eq!(
            key_action(&mods(false, false), keysyms::KEY_BackSpace),
            None
        );
        assert_eq!(key_action(&mods(true, false), keysyms::KEY_BackSpace), None);
        assert_eq!(key_action(&mods(true, true), keysyms::KEY_a), None);
    }
}
