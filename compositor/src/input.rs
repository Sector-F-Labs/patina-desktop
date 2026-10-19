//! Input from Smithay's input backends (winit when nested, libinput on the
//! hardware) into the compositor's seat, and the keys the compositor keeps
//! for itself.

use smithay::{
    backend::input::{
        AbsolutePositionEvent, Axis, AxisSource, ButtonState, Event, GestureBeginEvent,
        GestureSwipeUpdateEvent, InputBackend, InputEvent, KeyState, KeyboardKeyEvent,
        PointerAxisEvent, PointerButtonEvent, PointerMotionEvent,
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
    /// Restart the desktop under patina-session (Ctrl+Alt+Shift+Backspace):
    /// quit, to be started again (ADR-0046).
    Restart,
    /// The next keyboard layout (Ctrl+Space, as on macOS).
    NextLayout,
    /// Open the launcher (Super+Space, as Cmd+Space on macOS, or Super
    /// tapped on its own).
    Launcher,
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
    /// Activate the next tab of the bar (Alt+Tab), or the one before
    /// (Alt+Shift+Tab).
    CycleTabs { forward: bool },
    /// Grow the focused window in a direction (Super+Shift with an arrow).
    Grow(crate::maximize::Snap),
    /// Resize the focused window around its centre (Super+Shift+K/J grow
    /// and shrink it, L/H widen and narrow it).
    Resize(crate::maximize::Resize),
    /// Ask every window of the focused app to close (Super+Q): quitting it,
    /// politely, as Cmd+Q does on macOS.
    QuitApp,
    /// Lock the screen (Super+Escape).
    Lock,
    /// Take a screenshot (Super+Shift+3 or 4, Print).
    Screenshot(crate::screenshot::Shot),
    /// A Super key sent on to the focused window as Ctrl (ADR-0067):
    /// nothing more to do.
    SentAsCtrl,
}

/// The screenshot keys, from the key's base symbol so Shift and the layout
/// don't change which digit it is: Super+Shift+3 or Print takes the desktop,
/// Super+Shift+4 the focused window, as Cmd+Shift+3 and 4 do on a Mac.
pub fn screenshot_key(modifiers: &ModifiersState, base: u32) -> Option<KeyAction> {
    use crate::screenshot::Shot;
    let only_super_shift = modifiers.logo && modifiers.shift && !modifiers.ctrl && !modifiers.alt;
    let none = !modifiers.logo && !modifiers.shift && !modifiers.ctrl && !modifiers.alt;
    match base {
        keysyms::KEY_3 if only_super_shift => Some(KeyAction::Screenshot(Shot::Desktop)),
        keysyms::KEY_4 if only_super_shift => Some(KeyAction::Screenshot(Shot::Window)),
        keysyms::KEY_Print if none => Some(KeyAction::Screenshot(Shot::Desktop)),
        _ => None,
    }
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
        return Some(if modifiers.shift {
            KeyAction::Restart
        } else {
            KeyAction::Quit
        });
    }
    if sym == keysyms::KEY_space && !modifiers.alt && !modifiers.shift {
        if modifiers.logo && !modifiers.ctrl {
            return Some(KeyAction::Launcher);
        }
        if modifiers.ctrl && !modifiers.logo {
            return Some(KeyAction::NextLayout);
        }
    }
    // Alt+Tab steps along the bar's tabs; with Shift (which makes Tab
    // ISO_Left_Tab), backwards.
    if modifiers.alt && !modifiers.ctrl && !modifiers.logo {
        if sym == keysyms::KEY_Tab && !modifiers.shift {
            return Some(KeyAction::CycleTabs { forward: true });
        }
        if sym == keysyms::KEY_ISO_Left_Tab || (sym == keysyms::KEY_Tab && modifiers.shift) {
            return Some(KeyAction::CycleTabs { forward: false });
        }
    }
    // Ctrl+Super+Q locks, as Ctrl+Cmd+Q does on a Mac.
    if modifiers.logo
        && modifiers.ctrl
        && !modifiers.shift
        && !modifiers.alt
        && sym == keysyms::KEY_q
    {
        return Some(KeyAction::Lock);
    }
    if modifiers.logo && modifiers.shift && !modifiers.ctrl && !modifiers.alt {
        use crate::maximize::Resize;
        use crate::maximize::Snap;
        // With Shift held, the vim keys come as capitals.
        let action = match sym {
            keysyms::KEY_Up => Some(KeyAction::Grow(Snap::Up)),
            keysyms::KEY_Down => Some(KeyAction::Grow(Snap::Down)),
            keysyms::KEY_Left => Some(KeyAction::Grow(Snap::Left)),
            keysyms::KEY_Right => Some(KeyAction::Grow(Snap::Right)),
            keysyms::KEY_K => Some(KeyAction::Resize(Resize::Grow)),
            keysyms::KEY_J => Some(KeyAction::Resize(Resize::Shrink)),
            keysyms::KEY_L => Some(KeyAction::Resize(Resize::Wider)),
            keysyms::KEY_H => Some(KeyAction::Resize(Resize::Narrower)),
            _ => None,
        };
        if action.is_some() {
            return action;
        }
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
        if sym == keysyms::KEY_q {
            return Some(KeyAction::QuitApp);
        }
        if sym == keysyms::KEY_Escape {
            return Some(KeyAction::Lock);
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

/// Whether the compositor keeps `action` while a fullscreen window has the
/// keyboard: a game's own keys (Alt+letter, Super) go to the game, and only
/// stepping away, quitting or restarting the desktop, switching consoles
/// and the volume and brightness keys stay.
pub fn works_in_fullscreen(action: &KeyAction) -> bool {
    matches!(
        action,
        KeyAction::CycleTabs { .. }
            | KeyAction::Quit
            | KeyAction::Restart
            | KeyAction::SwitchVt(_)
            | KeyAction::Media(_)
    )
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
        let keycode = smithay::input::keyboard::Keycode::new(code + 8);
        let action = keyboard.input::<KeyAction, _>(
            self,
            keycode,
            state,
            SERIAL_COUNTER.next_serial(),
            time,
            |state, modifiers, handle| {
                sym = handle.modified_sym().raw();
                let base = handle
                    .raw_latin_sym_or_raw_current_sym()
                    .map_or(sym, |s| s.raw());
                let action = key_action(modifiers, sym)
                    .or_else(|| screenshot_key(modifiers, base))
                    .or_else(|| binding(&state.config.combos(), modifiers, base));
                // Locked, the keys go to the locker; only switching consoles
                // and the brightness and volume keys still work. Under a
                // fullscreen game, only the keys that get out of it.
                let action = action.filter(|a| {
                    if state.locked() {
                        matches!(a, KeyAction::SwitchVt(_) | KeyAction::Media(_))
                    } else if state.fullscreen_on_top().is_some() {
                        works_in_fullscreen(a)
                    } else {
                        true
                    }
                });
                if let (true, Some(action)) = (pressed, &action) {
                    return FilterResult::Intercept(action.clone());
                }
                // Super with any other key goes on as Ctrl (ADR-0067), but
                // not to the locker or a fullscreen game; its release too.
                let sent = if !pressed {
                    state.release_as_ctrl(handle, keycode, modifiers, time)
                } else {
                    action.is_none()
                        && !state.locked()
                        && state.fullscreen_on_top().is_none()
                        && state.send_as_ctrl(handle, keycode, modifiers, time)
                };
                if sent {
                    FilterResult::Intercept(KeyAction::SentAsCtrl)
                } else {
                    FilterResult::Forward
                }
            },
        );
        self.layout_maybe_changed();
        // Super tapped on its own (pressed and released with nothing in
        // between) opens the launcher; any other key or a click cancels it.
        let is_super = sym == keysyms::KEY_Super_L || sym == keysyms::KEY_Super_R;
        if pressed {
            self.super_tap = is_super;
        } else if is_super
            && std::mem::take(&mut self.super_tap)
            && !self.locked()
            && self.fullscreen_on_top().is_none()
        {
            self.open_launcher();
        }
        match action {
            Some(KeyAction::NextLayout) => {
                keyboard.with_xkb_state(self, |mut xkb| xkb.cycle_next_layout());
                self.layout_maybe_changed();
                None
            }
            Some(KeyAction::Reload) => {
                self.reload_config(true);
                None
            }
            Some(KeyAction::Launcher) => {
                self.open_launcher();
                None
            }
            Some(KeyAction::Restart) => {
                if let Err(e) = self.restart_desktop() {
                    eprintln!("patina-compositor: not restarting: {e}");
                }
                None
            }
            Some(KeyAction::SentAsCtrl) => None,
            Some(KeyAction::Lock) => {
                self.lock_screen();
                None
            }
            Some(KeyAction::QuitApp) => {
                self.quit_focused_app();
                None
            }
            Some(KeyAction::CycleTabs { forward }) => {
                self.cycle_tabs(forward);
                None
            }
            Some(KeyAction::Grow(toward)) => {
                self.grow(toward);
                None
            }
            Some(KeyAction::Resize(how)) => {
                self.resize_focused(how);
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
            Some(KeyAction::Screenshot(shot)) => {
                self.screenshot(shot);
                None
            }
            other => other,
        }
    }

    /// Opens the configured launcher, unless it's set to "".
    fn open_launcher(&mut self) {
        if !self.config.launcher.is_empty() {
            self.spawn_client(&self.config.launcher.clone());
        }
    }

    /// Moves the pointer by `delta`, kept on screen: a mouse or touchpad
    /// moving it, or a three-finger drag.
    pub fn move_pointer_by(&mut self, delta: Point<f64, Logical>, time: u32) {
        let target = self.pointer_target(delta);
        self.pointer_motion(target, time);
    }

    /// Where the pointer would be moved by `delta`, kept on screen.
    pub fn pointer_target(&self, delta: Point<f64, Logical>) -> Point<f64, Logical> {
        let screen = self.screen();
        let current = self
            .seat
            .get_pointer()
            .map(|p| p.current_location())
            .unwrap_or_default();
        let moved = current + delta;
        Point::from((
            moved.x.clamp(
                screen.loc.x as f64,
                (screen.loc.x + screen.size.w - 1) as f64,
            ),
            moved.y.clamp(
                screen.loc.y as f64,
                (screen.loc.y + screen.size.h - 1) as f64,
            ),
        ))
    }
}

/// Feeds one event into the seat. Returns a key action for the backend to
/// carry out (switching consoles is the session's job).
pub fn handle<B: InputBackend>(state: &mut State, event: InputEvent<B>) -> Option<KeyAction> {
    if !matches!(
        event,
        InputEvent::DeviceAdded { .. } | InputEvent::DeviceRemoved { .. }
    ) {
        state.input_activity();
    }
    match event {
        InputEvent::PointerMotion { event } => {
            state.one_finger_moved(event.delta());
            state.pointer_moved(
                event.delta(),
                event.delta_unaccel(),
                event.time(),
                event.time_msec(),
            );
        }
        InputEvent::PointerMotionAbsolute { event } => {
            let screen = state.screen();
            let pos = event.position_transformed(screen.size) + screen.loc.to_f64();
            state.pointer_motion(pos, event.time_msec());
        }
        InputEvent::PointerButton { event } => {
            // A click between a drag's strokes drops what's dragged first.
            if state.drag_between_strokes() {
                state.end_drag();
            }
            state.pointer_button(
                event.button_code(),
                event.state() == ButtonState::Pressed,
                event.time_msec(),
            )
        }
        // Three fingers drag (ADR-0031); other swipes aren't used yet.
        InputEvent::GestureSwipeBegin { event } => {
            state.swipe_begin(event.fingers(), event.time_msec());
        }
        InputEvent::GestureSwipeUpdate { event } => {
            state.swipe_update(event.delta(), event.time_msec());
        }
        InputEvent::GestureSwipeEnd { .. } => state.swipe_end(),
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
    fn mac_keys_take_screenshots() {
        use crate::screenshot::Shot;
        let super_shift = ModifiersState {
            logo: true,
            shift: true,
            ..Default::default()
        };
        assert_eq!(
            screenshot_key(&super_shift, keysyms::KEY_3),
            Some(KeyAction::Screenshot(Shot::Desktop))
        );
        assert_eq!(
            screenshot_key(&super_shift, keysyms::KEY_4),
            Some(KeyAction::Screenshot(Shot::Window))
        );
        assert_eq!(
            screenshot_key(&mods(false, false), keysyms::KEY_Print),
            Some(KeyAction::Screenshot(Shot::Desktop))
        );
        let logo = ModifiersState {
            logo: true,
            ..Default::default()
        };
        assert_eq!(screenshot_key(&logo, keysyms::KEY_3), None);
        assert_eq!(screenshot_key(&mods(false, false), keysyms::KEY_3), None);
    }

    #[test]
    fn super_space_opens_the_launcher_and_ctrl_space_switches_layout() {
        let logo = ModifiersState {
            logo: true,
            ..Default::default()
        };
        let ctrl = ModifiersState {
            ctrl: true,
            ..Default::default()
        };
        let both = ModifiersState {
            ctrl: true,
            logo: true,
            ..Default::default()
        };
        assert_eq!(
            key_action(&logo, keysyms::KEY_space),
            Some(KeyAction::Launcher)
        );
        assert_eq!(
            key_action(&ctrl, keysyms::KEY_space),
            Some(KeyAction::NextLayout)
        );
        assert_eq!(key_action(&both, keysyms::KEY_space), None);
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
    fn ctrl_super_q_and_super_escape_lock() {
        let logo = ModifiersState {
            logo: true,
            ..Default::default()
        };
        let ctrl_logo = ModifiersState {
            logo: true,
            ctrl: true,
            ..Default::default()
        };
        assert_eq!(
            key_action(&ctrl_logo, keysyms::KEY_q),
            Some(KeyAction::Lock)
        );
        assert_eq!(key_action(&logo, keysyms::KEY_q), Some(KeyAction::QuitApp));
        assert_eq!(
            key_action(&logo, keysyms::KEY_l),
            Some(KeyAction::Snap(crate::maximize::Snap::Right))
        );
        assert_eq!(
            key_action(&logo, keysyms::KEY_Escape),
            Some(KeyAction::Lock)
        );
    }

    #[test]
    fn super_shift_arrows_grow_and_vim_keys_resize() {
        use crate::maximize::{Resize, Snap};
        let keys = ModifiersState {
            logo: true,
            shift: true,
            ..Default::default()
        };
        for (sym, toward) in [
            (keysyms::KEY_Up, Snap::Up),
            (keysyms::KEY_Down, Snap::Down),
            (keysyms::KEY_Left, Snap::Left),
            (keysyms::KEY_Right, Snap::Right),
        ] {
            assert_eq!(key_action(&keys, sym), Some(KeyAction::Grow(toward)));
        }
        for (sym, how) in [
            (keysyms::KEY_K, Resize::Grow),
            (keysyms::KEY_J, Resize::Shrink),
            (keysyms::KEY_L, Resize::Wider),
            (keysyms::KEY_H, Resize::Narrower),
        ] {
            assert_eq!(key_action(&keys, sym), Some(KeyAction::Resize(how)));
        }
    }

    #[test]
    fn alt_tab_cycles_and_shift_goes_back() {
        let alt = mods(false, true);
        assert_eq!(
            key_action(&alt, keysyms::KEY_Tab),
            Some(KeyAction::CycleTabs { forward: true })
        );
        let alt_shift = ModifiersState {
            alt: true,
            shift: true,
            ..Default::default()
        };
        assert_eq!(
            key_action(&alt_shift, keysyms::KEY_ISO_Left_Tab),
            Some(KeyAction::CycleTabs { forward: false })
        );
        assert_eq!(key_action(&mods(false, false), keysyms::KEY_Tab), None);
    }

    #[test]
    fn a_fullscreen_game_keeps_its_keys() {
        let alt = mods(false, true);
        let combos = [(
            crate::config::combo("Alt+W").unwrap(),
            "whatsapp".to_string(),
        )];
        let run = binding(&combos, &alt, keysyms::KEY_w).unwrap();
        assert!(!works_in_fullscreen(&run));
        let alt_tab = key_action(&alt, keysyms::KEY_Tab).unwrap();
        assert!(works_in_fullscreen(&alt_tab));
        let logo = ModifiersState {
            logo: true,
            ..Default::default()
        };
        for sym in [keysyms::KEY_q, keysyms::KEY_Left, keysyms::KEY_1] {
            assert!(!works_in_fullscreen(&key_action(&logo, sym).unwrap()));
        }
        let ctrl_alt = mods(true, true);
        assert!(works_in_fullscreen(
            &key_action(&ctrl_alt, keysyms::KEY_BackSpace).unwrap()
        ));
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
    fn super_w_and_t_go_to_the_app() {
        let logo = ModifiersState {
            logo: true,
            ..Default::default()
        };
        assert_eq!(key_action(&logo, keysyms::KEY_w), None);
        assert_eq!(key_action(&logo, keysyms::KEY_t), None);
    }

    #[test]
    fn ctrl_alt_backspace_quits_and_backspace_alone_types() {
        assert_eq!(
            key_action(&mods(true, true), keysyms::KEY_BackSpace),
            Some(KeyAction::Quit)
        );
        let shift = ModifiersState {
            shift: true,
            ..mods(true, true)
        };
        assert_eq!(
            key_action(&shift, keysyms::KEY_BackSpace),
            Some(KeyAction::Restart)
        );
        assert_eq!(
            key_action(&mods(false, false), keysyms::KEY_BackSpace),
            None
        );
        assert_eq!(key_action(&mods(true, false), keysyms::KEY_BackSpace), None);
        assert_eq!(key_action(&mods(true, true), keysyms::KEY_a), None);
    }
}
