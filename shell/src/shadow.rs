//! A shadow for a layer surface (ADR-0113), asked of the compositor through
//! KDE's shadow protocol, the one Patina apps ask through (Patina's ADR-0030):
//! the eight tiles around a window that patina-ui draws, handed over as shm
//! buffers with how far they reach. The compositor draws them outside the
//! surface, so the surface stays the size of what it shows, and its exclusive
//! zone and anchors are untouched. A compositor without the protocol draws
//! nothing.

pub use patina::frame::shadow::Style;
use patina::frame::shadow::tiles;
use smithay_client_toolkit::{
    reexports::client::{
        QueueHandle,
        protocol::{wl_shm, wl_surface::WlSurface},
    },
    shm::{
        Shm,
        slot::{Buffer, SlotPool},
    },
};
use wayland_protocols_plasma::shadow::client::{
    org_kde_kwin_shadow::OrgKdeKwinShadow, org_kde_kwin_shadow_manager::OrgKdeKwinShadowManager,
};

use crate::host::{Host, Shell};
use crate::scale::NoEvents;

/// What keeps a surface's shadow: the protocol object, and the tiles' buffers
/// until the surface goes. Dropping it takes the shadow away, as the protocol
/// says destroying the object does.
pub(crate) struct Hold {
    shadow: OrgKdeKwinShadow,
    _pool: SlotPool,
    _buffers: Vec<Buffer>,
}

impl Drop for Hold {
    fn drop(&mut self) {
        self.shadow.destroy();
    }
}

/// Gives `surface` the shadow of `style`.
pub(crate) fn attach<S: Shell>(
    manager: &OrgKdeKwinShadowManager,
    shm: &Shm,
    qh: &QueueHandle<Host<S>>,
    surface: &WlSurface,
    style: Style,
    inset: (i32, i32, i32, i32),
) -> Result<Hold, String> {
    let (tiles, offsets) = tiles(style);
    let total: usize = tiles.iter().map(|(w, h, _)| (w * h * 4) as usize).sum();
    // A pool of its own, so nothing the surface draws later can land on the
    // tiles before the compositor has read them.
    let mut pool = SlotPool::new(total, shm).map_err(|e| e.to_string())?;
    let mut buffers = Vec::with_capacity(tiles.len());
    for (w, h, bytes) in &tiles {
        let (buffer, canvas) = pool
            .create_buffer(*w, *h, w * 4, wl_shm::Format::Argb8888)
            .map_err(|e| e.to_string())?;
        canvas[..bytes.len()].copy_from_slice(bytes);
        buffers.push(buffer);
    }
    let shadow = manager.create(surface, qh, NoEvents);
    let [
        left,
        top_left,
        top,
        top_right,
        right,
        bottom_right,
        bottom,
        bottom_left,
    ] = [0, 1, 2, 3, 4, 5, 6, 7].map(|i| buffers[i].wl_buffer());
    shadow.attach_left(left);
    shadow.attach_top_left(top_left);
    shadow.attach_top(top);
    shadow.attach_top_right(top_right);
    shadow.attach_right(right);
    shadow.attach_bottom_right(bottom_right);
    shadow.attach_bottom(bottom);
    shadow.attach_bottom_left(bottom_left);
    // How far it reaches outside the surface: less where the shape drawn is
    // inside it, by `inset` (top, right, bottom, left).
    let (inset_top, inset_right, inset_bottom, inset_left) = inset;
    shadow.set_left_offset((offsets[0] - inset_left) as f64);
    shadow.set_top_offset((offsets[1] - inset_top) as f64);
    shadow.set_right_offset((offsets[2] - inset_right) as f64);
    shadow.set_bottom_offset((offsets[3] - inset_bottom) as f64);
    shadow.commit();
    Ok(Hold {
        shadow,
        _pool: pool,
        _buffers: buffers,
    })
}
