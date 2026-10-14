//! wlr-foreign-toplevel-management: tells clients such as patina-bar (or
//! waybar's taskbar) which windows are open, their titles and app ids and
//! which is active, and lets them activate or close one (ADR-0012).
//!
//! It works as a diff: after each pass of the event loop, [`refresh`]
//! compares the windows with what clients were last told and sends only
//! what changed.

use smithay::{
    desktop::Window,
    reexports::{
        wayland_protocols_wlr::foreign_toplevel::v1::server::{
            zwlr_foreign_toplevel_handle_v1::{self, ZwlrForeignToplevelHandleV1},
            zwlr_foreign_toplevel_manager_v1::{self, ZwlrForeignToplevelManagerV1},
        },
        wayland_server::{
            Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource,
            protocol::wl_surface::WlSurface,
        },
    },
    wayland::{compositor::with_states, shell::xdg::XdgToplevelSurfaceData},
};

use crate::state::State;

const VERSION: u32 = 3;

#[derive(Default)]
pub struct ForeignToplevels {
    managers: Vec<ZwlrForeignToplevelManagerV1>,
    /// In the order the windows opened, which is the order clients see.
    entries: Vec<Entry>,
}

/// One window, and what its clients were last told about it.
struct Entry {
    surface: WlSurface,
    handles: Vec<ZwlrForeignToplevelHandleV1>,
    title: String,
    app_id: String,
    activated: bool,
}

/// A handle's link back to its window.
pub struct HandleData {
    surface: WlSurface,
}

impl ForeignToplevels {
    pub fn new(dh: &DisplayHandle) -> Self {
        dh.create_global::<State, ZwlrForeignToplevelManagerV1, ()>(VERSION, ());
        Self::default()
    }
}

/// What a client should see of `surface`'s window.
fn describe(surface: &WlSurface) -> (String, String) {
    with_states(surface, |states| {
        states
            .data_map
            .get::<XdgToplevelSurfaceData>()
            .and_then(|d| d.lock().ok())
            .map(|d| {
                (
                    d.title.clone().unwrap_or_default(),
                    d.app_id.clone().unwrap_or_default(),
                )
            })
            .unwrap_or_default()
    })
}

/// Sends everything about `entry` to one handle, then `done`.
fn send_all(handle: &ZwlrForeignToplevelHandleV1, entry: &Entry) {
    handle.title(entry.title.clone());
    handle.app_id(entry.app_id.clone());
    send_state(handle, entry.activated);
    handle.done();
}

fn send_state(handle: &ZwlrForeignToplevelHandleV1, activated: bool) {
    let mut state = Vec::new();
    if activated {
        state.extend_from_slice(
            &(zwlr_foreign_toplevel_handle_v1::State::Activated as u32).to_ne_bytes(),
        );
    }
    handle.state(state);
}

/// Makes a handle for `entry` on `manager`'s client and tells it everything.
fn announce(dh: &DisplayHandle, manager: &ZwlrForeignToplevelManagerV1, entry: &mut Entry) {
    let Some(client) = manager.client() else {
        return;
    };
    let data = HandleData {
        surface: entry.surface.clone(),
    };
    let Ok(handle) = client.create_resource::<ZwlrForeignToplevelHandleV1, _, State>(
        dh,
        manager.version(),
        data,
    ) else {
        return;
    };
    manager.toplevel(&handle);
    send_all(&handle, entry);
    entry.handles.push(handle);
}

/// Brings clients up to date with `windows`; `focused` is the surface with
/// the keyboard, which is the active window.
pub fn refresh(state: &mut State) {
    let focused = state.seat.get_keyboard().and_then(|k| k.current_focus());
    let windows: Vec<WlSurface> = state
        .space
        .elements()
        .filter_map(Window::toplevel)
        .map(|t| t.wl_surface().clone())
        .collect();
    let dh = state.display_handle.clone();
    let this = &mut state.toplevels;

    this.entries.retain(|entry| {
        let open = windows.contains(&entry.surface);
        if !open {
            for handle in &entry.handles {
                handle.closed();
            }
        }
        open
    });

    for surface in windows {
        let (title, app_id) = describe(&surface);
        let activated = focused.as_ref() == Some(&surface);
        match this.entries.iter_mut().find(|e| e.surface == surface) {
            None => {
                let mut entry = Entry {
                    surface,
                    handles: Vec::new(),
                    title,
                    app_id,
                    activated,
                };
                for manager in &this.managers {
                    announce(&dh, manager, &mut entry);
                }
                this.entries.push(entry);
            }
            Some(entry) => {
                let title_changed = entry.title != title;
                let app_id_changed = entry.app_id != app_id;
                let state_changed = entry.activated != activated;
                if !(title_changed || app_id_changed || state_changed) {
                    continue;
                }
                entry.title = title;
                entry.app_id = app_id;
                entry.activated = activated;
                for handle in &entry.handles {
                    if title_changed {
                        handle.title(entry.title.clone());
                    }
                    if app_id_changed {
                        handle.app_id(entry.app_id.clone());
                    }
                    if state_changed {
                        send_state(handle, entry.activated);
                    }
                    handle.done();
                }
            }
        }
    }
}

impl GlobalDispatch<ZwlrForeignToplevelManagerV1, ()> for State {
    fn bind(
        state: &mut Self,
        dh: &DisplayHandle,
        _: &Client,
        resource: New<ZwlrForeignToplevelManagerV1>,
        _: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        let manager = data_init.init(resource, ());
        for entry in &mut state.toplevels.entries {
            announce(dh, &manager, entry);
        }
        state.toplevels.managers.push(manager);
    }
}

impl Dispatch<ZwlrForeignToplevelManagerV1, ()> for State {
    fn request(
        state: &mut Self,
        _: &Client,
        manager: &ZwlrForeignToplevelManagerV1,
        request: zwlr_foreign_toplevel_manager_v1::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        if let zwlr_foreign_toplevel_manager_v1::Request::Stop = request {
            state.toplevels.managers.retain(|m| m != manager);
            manager.finished();
        }
    }

    fn destroyed(
        state: &mut Self,
        _: smithay::reexports::wayland_server::backend::ClientId,
        manager: &ZwlrForeignToplevelManagerV1,
        _: &(),
    ) {
        state.toplevels.managers.retain(|m| m != manager);
    }
}

impl Dispatch<ZwlrForeignToplevelHandleV1, HandleData> for State {
    fn request(
        state: &mut Self,
        _: &Client,
        handle: &ZwlrForeignToplevelHandleV1,
        request: zwlr_foreign_toplevel_handle_v1::Request,
        data: &HandleData,
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        use zwlr_foreign_toplevel_handle_v1::Request;
        match request {
            Request::Activate { .. } => state.activate_toplevel(&data.surface),
            Request::Close => state.close_toplevel(&data.surface),
            Request::Destroy => forget(state, handle),
            // Floating windows with no maximise, minimise or fullscreen
            // yet, and no use for a taskbar's rectangle.
            _ => {}
        }
    }

    fn destroyed(
        state: &mut Self,
        _: smithay::reexports::wayland_server::backend::ClientId,
        handle: &ZwlrForeignToplevelHandleV1,
        _: &HandleData,
    ) {
        forget(state, handle);
    }
}

fn forget(state: &mut State, handle: &ZwlrForeignToplevelHandleV1) {
    for entry in &mut state.toplevels.entries {
        entry.handles.retain(|h| h != handle);
    }
}
