//! What has the keyboard: a Wayland surface (a window, a layer surface, the
//! locker) or an X11 window under Xwayland (ADR-0040). An X window needs the
//! X server told as well (`SetInputFocus`), which Smithay's `X11Surface`
//! does as a keyboard target; its Wayland surface gets the keys.

use std::borrow::Cow;

use smithay::{
    backend::input::KeyState,
    input::{
        Seat,
        keyboard::{KeyboardTarget, KeysymHandle, ModifiersState},
    },
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{IsAlive, Serial},
    wayland::seat::WaylandFocus,
    xwayland::X11Surface,
};

use crate::state::State;

#[derive(Debug, Clone, PartialEq)]
pub enum FocusTarget {
    Surface(WlSurface),
    X11(X11Surface),
}

impl FocusTarget {
    /// Whether this is `surface`, or the X window drawn on it.
    pub fn is(&self, surface: &WlSurface) -> bool {
        self.wl_surface().as_deref() == Some(surface)
    }
}

impl From<WlSurface> for FocusTarget {
    fn from(surface: WlSurface) -> Self {
        FocusTarget::Surface(surface)
    }
}

impl IsAlive for FocusTarget {
    fn alive(&self) -> bool {
        match self {
            FocusTarget::Surface(s) => s.alive(),
            FocusTarget::X11(x) => x.alive(),
        }
    }
}

impl WaylandFocus for FocusTarget {
    fn wl_surface(&self) -> Option<Cow<'_, WlSurface>> {
        match self {
            FocusTarget::Surface(s) => Some(Cow::Borrowed(s)),
            FocusTarget::X11(x) => x.wl_surface().map(Cow::Owned),
        }
    }
}

impl KeyboardTarget<State> for FocusTarget {
    fn enter(
        &self,
        seat: &Seat<State>,
        data: &mut State,
        keys: Vec<KeysymHandle<'_>>,
        serial: Serial,
    ) {
        match self {
            FocusTarget::Surface(s) => KeyboardTarget::enter(s, seat, data, keys, serial),
            FocusTarget::X11(x) => KeyboardTarget::enter(x, seat, data, keys, serial),
        }
    }

    fn leave(&self, seat: &Seat<State>, data: &mut State, serial: Serial) {
        match self {
            FocusTarget::Surface(s) => KeyboardTarget::leave(s, seat, data, serial),
            FocusTarget::X11(x) => KeyboardTarget::leave(x, seat, data, serial),
        }
    }

    fn key(
        &self,
        seat: &Seat<State>,
        data: &mut State,
        key: KeysymHandle<'_>,
        state: KeyState,
        serial: Serial,
        time: u32,
    ) {
        match self {
            FocusTarget::Surface(s) => KeyboardTarget::key(s, seat, data, key, state, serial, time),
            FocusTarget::X11(x) => KeyboardTarget::key(x, seat, data, key, state, serial, time),
        }
    }

    fn modifiers(
        &self,
        seat: &Seat<State>,
        data: &mut State,
        modifiers: ModifiersState,
        serial: Serial,
    ) {
        match self {
            FocusTarget::Surface(s) => KeyboardTarget::modifiers(s, seat, data, modifiers, serial),
            FocusTarget::X11(x) => KeyboardTarget::modifiers(x, seat, data, modifiers, serial),
        }
    }
}
