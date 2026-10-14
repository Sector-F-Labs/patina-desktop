//! Fractional scaling (ADR-0018): wp_fractional_scale_v1 tells each surface
//! the output's exact scale (1.5 on a 14" 1920x1200 panel), and
//! wp_viewporter lets the client attach a buffer drawn at that scale and say
//! what logical size it covers. Without them, clients only learn the
//! rounded-up integer scale, draw at 2x, and the compositor shrinks the
//! result, which blurs text. Smithay's renderer already honours viewports.

use smithay::{
    delegate_fractional_scale, delegate_viewporter,
    desktop::layer_map_for_output,
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    wayland::{
        compositor::{SurfaceData, with_states},
        fractional_scale::{FractionalScaleHandler, with_fractional_scale},
    },
};

use crate::state::State;

fn prefer(states: &SurfaceData, scale: f64) {
    with_fractional_scale(states, |fs| fs.set_preferred_scale(scale));
}

impl FractionalScaleHandler for State {
    fn new_fractional_scale(&mut self, surface: WlSurface) {
        let scale = self.output.current_scale().fractional_scale();
        with_states(&surface, |states| prefer(states, scale));
    }
}

impl State {
    /// Tells every surface the output's scale again, after it changed (the
    /// nested window moved to a monitor with another scale).
    pub fn update_preferred_scale(&self) {
        let scale = self.output.current_scale().fractional_scale();
        for window in self.space.elements() {
            window.with_surfaces(|_, states| prefer(states, scale));
        }
        for layer in layer_map_for_output(&self.output).layers() {
            layer.with_surfaces(|_, states| prefer(states, scale));
        }
    }
}

delegate_fractional_scale!(State);
delegate_viewporter!(State);
