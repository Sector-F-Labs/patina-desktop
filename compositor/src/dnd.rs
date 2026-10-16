//! Drag and drop between clients: Smithay's data-device does the protocol;
//! this draws what's being dragged. A client starting a drag may give a
//! surface to show under the pointer (a file's icon, a picture); it's drawn
//! at the pointer, moved by the offsets the client attaches, in every
//! backend, until the drop.

use smithay::{
    backend::renderer::{
        ImportAll, Renderer, Texture,
        element::{
            Kind,
            surface::{WaylandSurfaceRenderElement, render_elements_from_surface_tree},
        },
    },
    desktop::utils::send_frames_surface_tree,
    input::Seat,
    output::Output,
    reexports::wayland_server::protocol::{wl_data_source::WlDataSource, wl_surface::WlSurface},
    utils::{Logical, Point},
    wayland::{
        compositor::{SurfaceAttributes, with_states},
        selection::data_device::ClientDndGrabHandler,
    },
};

use crate::state::State;

/// The surface a client drags, and where it sits from the pointer.
#[derive(Debug, Clone)]
pub struct Icon {
    pub surface: WlSurface,
    offset: Point<i32, Logical>,
}

impl ClientDndGrabHandler for State {
    fn started(&mut self, _: Option<WlDataSource>, icon: Option<WlSurface>, _: Seat<Self>) {
        self.dnd_icon = icon.map(|surface| Icon {
            surface,
            offset: (0, 0).into(),
        });
        self.needs_redraw = true;
    }

    fn dropped(&mut self, _: Option<WlSurface>, _: bool, _: Seat<Self>) {
        self.dnd_icon = None;
        self.needs_redraw = true;
    }
}

impl State {
    /// A commit to the drag icon moves it by the offset it attached with.
    pub fn dnd_icon_commit(&mut self, surface: &WlSurface) {
        let Some(icon) = self.dnd_icon.as_mut().filter(|i| i.surface == *surface) else {
            return;
        };
        let delta = with_states(surface, |states| {
            states
                .cached_state
                .get::<SurfaceAttributes>()
                .current()
                .buffer_delta
                .take()
        });
        icon.offset += delta.unwrap_or_default();
    }

    /// The icon being dragged and where it goes: the pointer plus its
    /// offset. None while locked.
    pub fn dnd_icon_at(&self) -> Option<(WlSurface, Point<f64, Logical>)> {
        if self.locked() {
            return None;
        }
        let icon = self.dnd_icon.as_ref()?;
        let pointer = self.seat.get_pointer()?.current_location();
        Some((icon.surface.clone(), pointer + icon.offset.to_f64()))
    }
}

/// The drag icon's elements, at `at` in logical points.
pub fn elements<R>(
    renderer: &mut R,
    icon: Option<&(WlSurface, Point<f64, Logical>)>,
    scale: f64,
) -> Vec<WaylandSurfaceRenderElement<R>>
where
    R: Renderer + ImportAll,
    R::TextureId: Clone + Texture + 'static,
{
    let Some((surface, at)) = icon else {
        return Vec::new();
    };
    let at = at.to_physical(scale).to_i32_round();
    render_elements_from_surface_tree(renderer, surface, at, scale, 1.0, Kind::Unspecified)
}

/// Tells the drag icon to draw its next frame.
pub fn send_frames(
    icon: Option<&(WlSurface, Point<f64, Logical>)>,
    output: &Output,
    time: std::time::Duration,
) {
    if let Some((surface, _)) = icon {
        send_frames_surface_tree(
            surface,
            output,
            time,
            Some(std::time::Duration::ZERO),
            |_, _| Some(output.clone()),
        );
    }
}
