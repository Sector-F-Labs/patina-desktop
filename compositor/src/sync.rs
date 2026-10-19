//! Waiting for clients' GPU buffers to be drawn before showing them. A
//! client's commit is held back until its buffer is ready: by the acquire
//! point it gives over linux-drm-syncobj (explicit sync, offered on
//! hardware when the DRM device can signal syncobjs on an eventfd), or by
//! the dmabuf's own fence (implicit sync). NVIDIA's driver puts no implicit
//! fences on dmabufs, so without explicit sync Chromium and Electron apps
//! were shown half-drawn, and flickered.

use smithay::{
    delegate_drm_syncobj,
    reexports::{
        calloop::Interest,
        wayland_server::{Resource, protocol::wl_surface::WlSurface},
    },
    wayland::{
        compositor::{
            BufferAssignment, CompositorHandler, SurfaceAttributes, add_blocker,
            add_pre_commit_hook, with_states,
        },
        dmabuf::get_dmabuf,
        drm_syncobj::{DrmSyncobjCachedState, DrmSyncobjHandler, DrmSyncobjState},
    },
};

use crate::state::State;

impl DrmSyncobjHandler for State {
    fn drm_syncobj_state(&mut self) -> Option<&mut DrmSyncobjState> {
        self.udev.as_mut()?.syncobj_state.as_mut()
    }
}

delegate_drm_syncobj!(State);

/// Holds each commit of `surface` with a new dmabuf until it's drawn.
pub fn wait_for_buffers(surface: &WlSurface) {
    add_pre_commit_hook::<State, _>(surface, |state, _, surface| {
        let (dmabuf, acquire) = with_states(surface, |states| {
            let acquire = states
                .cached_state
                .get::<DrmSyncobjCachedState>()
                .pending()
                .acquire_point
                .clone();
            let dmabuf = match states
                .cached_state
                .get::<SurfaceAttributes>()
                .pending()
                .buffer
                .as_ref()
            {
                Some(BufferAssignment::NewBuffer(buffer)) => get_dmabuf(buffer).cloned().ok(),
                _ => None,
            };
            (dmabuf, acquire)
        });
        let Some(dmabuf) = dmabuf else { return };
        let Some(client) = surface.client() else { return };
        // The commit goes ahead once the source fires.
        let release = move |state: &mut State| {
            let dh = state.display_handle.clone();
            state
                .client_compositor_state(&client)
                .blocker_cleared(state, &dh);
        };
        if let Some((blocker, source)) = acquire.and_then(|p| p.generate_blocker().ok()) {
            let release = release.clone();
            if state
                .loop_handle
                .insert_source(source, move |_, _, state| {
                    release(state);
                    Ok(())
                })
                .is_ok()
            {
                add_blocker(surface, blocker);
                return;
            }
        }
        if let Ok((blocker, source)) = dmabuf.generate_blocker(Interest::READ)
            && state
                .loop_handle
                .insert_source(source, move |_, _, state| {
                    release(state);
                    Ok(())
                })
                .is_ok()
        {
            add_blocker(surface, blocker);
        }
    });
}
