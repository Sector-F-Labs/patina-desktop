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
#[derive(Debug, Clone, PartialEq, Eq)]
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
    /// Bring forward the window in the bar's `n`th tab (Super+1 to 9).
    Window(usize),
    /// A [bindings] command from the config.
    Run(String),
    /// Maximise, restore or tile the focused window (Super with an arrow
    /// or a vim key).
    Snap(crate::maximize::Snap),
    /// Ask the focused window to close (Super+W), as its close button does.
    CloseWindow,
    /// Ask every window of the focused app to close (Super+Q): quitting it,
    /// politely, as Cmd+Q does on macOS.
    QuitApp,
}

/// The [bindings] command for a key, if one matches: exact modifiers, and
/// the key's base symbol (`base`, unshifted on the first layout), so Shift
/// or another layout don't change which key it is.
pub fn binding(
    combos: &[(crate::config::Combo, String)],
    modifiers: &ModifiersState,
    base: u32,
) -> Option<KeyAction> {
    combos
        .iter()
        .find(|(c, _)| {
            c.logo == modifiers.logo
                && c.alt == modifiers.alt
                && c.ctrl == modifiers.ctrl
                && c.shift == modifiers.shift
                && c.sym == base
        })
        .map(|(_, command)| KeyAction::Run(command.clone()))
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
    if modifiers.logo && !modifiers.shift && !modifiers.ctrl && !modifiers.alt {
        use crate::maximize::Snap;
        let snap = match sym {
            keysyms::KEY_Up | keysyms::KEY_k => Some(Snap::Up),
            keysyms::KEY_Down | keysyms::KEY_j => Some(Snap::Down),
            keysyms::KEY_Left | keysyms::KEY_h => Some(Snap::Left),
            keysyms::KEY_Right | keysyms::KEY_l => Some(Snap::Right),
            _ => None,
        };
        if let Some(snap) = snap {
            return Some(KeyAction::Snap(snap));
        }
        if sym == keysyms::KEY_w {
            return Some(KeyAction::CloseWindow);
        }
        if sym == keysyms::KEY_q {
            return Some(KeyAction::QuitApp);
        }
    }
    if modifiers.logo
        && !modifiers.shift
        && !modifiers.ctrl
        && (keysyms::KEY_1..=keysyms::KEY_9).contains(&sym)
    {
        return Some(KeyAction::Window((sym - keysyms::KEY_1 + 1) as usize));
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
            |state, modifiers, handle| {
                sym = handle.modified_sym().raw();
                let base = handle
                    .raw_latin_sym_or_raw_current_sym()
                    .map_or(sym, |s| s.raw());
                let action = key_action(modifiers, sym)
                    .or_else(|| binding(&state.config.combos(), modifiers, base));
                match (pressed, action) {
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
            Some(KeyAction::CloseWindow) => {
                if let Some(surface) = self
                    .focused_window()
                    .and_then(|w| w.toplevel().map(|t| t.wl_surface().clone()))
                {
                    self.close_toplevel(&surface);
                }
                None
            }
            Some(KeyAction::QuitApp) => {
                self.quit_focused_app();
                None
            }
            Some(KeyAction::Snap(snap)) => {
                self.snap(snap);
                None
            }
            Some(KeyAction::Run(command)) => {
                self.spawn_client(&command);
                None
            }
            Some(KeyAction::Window(n)) => {
                if let Some(surface) = self.toplevels.nth(n) {
                    self.activate_toplevel(&surface);
                }
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
    fn super_and_a_digit_picks_a_tab() {
        let logo = ModifiersState {
            logo: true,
            ..Default::default()
        };
        assert_eq!(
            key_action(&logo, keysyms::KEY_1),
            Some(KeyAction::Window(1))
        );
        assert_eq!(
            key_action(&logo, keysyms::KEY_9),
            Some(KeyAction::Window(9))
        );
        assert_eq!(key_action(&logo, keysyms::KEY_0), None);
        assert_eq!(key_action(&mods(false, false), keysyms::KEY_1), None);
    }

    #[test]
    fn bindings_match_exact_modifiers_on_the_base_key() {
        let combos = vec![(
            crate::config::combo("Alt+T").unwrap(),
            "teleprinter".to_string(),
        )];
        let alt = mods(false, true);
        assert_eq!(
            binding(&combos, &alt, keysyms::KEY_t),
            Some(KeyAction::Run("teleprinter".into()))
        );
        assert_eq!(binding(&combos, &mods(true, true), keysyms::KEY_t), None);
        assert_eq!(binding(&combos, &alt, keysyms::KEY_b), None);
    }

    #[test]
    fn super_with_arrows_or_vim_keys_snaps() {
        use crate::maximize::Snap;
        let logo = ModifiersState {
            logo: true,
            ..Default::default()
        };
        for (sym, snap) in [
            (keysyms::KEY_Up, Snap::Up),
            (keysyms::KEY_k, Snap::Up),
            (keysyms::KEY_Down, Snap::Down),
            (keysyms::KEY_j, Snap::Down),
            (keysyms::KEY_Left, Snap::Left),
            (keysyms::KEY_h, Snap::Left),
            (keysyms::KEY_Right, Snap::Right),
            (keysyms::KEY_l, Snap::Right),
        ] {
            assert_eq!(key_action(&logo, sym), Some(KeyAction::Snap(snap)));
        }
        assert_eq!(key_action(&mods(false, false), keysyms::KEY_Up), None);
    }

    #[test]
    fn super_q_quits() {
        let logo = ModifiersState {
            logo: true,
            ..Default::default()
        };
        assert_eq!(key_action(&logo, keysyms::KEY_q), Some(KeyAction::QuitApp));
    }

    #[test]
    fn super_w_closes() {
        let logo = ModifiersState {
            logo: true,
            ..Default::default()
        };
        assert_eq!(
            key_action(&logo, keysyms::KEY_w),
            Some(KeyAction::CloseWindow)
        );
        assert_eq!(key_action(&mods(false, false), keysyms::KEY_w), None);
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
