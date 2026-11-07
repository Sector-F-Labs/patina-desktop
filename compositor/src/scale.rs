//! Fractional scaling (ADR-0018): wp_fractional_scale_v1 tells each surface
//! the output's exact scale (1.5 on a 14" 1920x1200 panel), and
//! wp_viewporter lets the client attach a buffer drawn at that scale and say
//! what logical size it covers. Without them, clients only learn the
//! rounded-up integer scale, draw at 2x, and the compositor shrinks the
//! result, which blurs text. Smithay's renderer already honours viewports.

use smithay::{
    delegate_fractional_scale, delegate_viewporter,
    desktop::{WindowSurfaceType, layer_map_for_output},
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
        let scale = self.scale_for(&surface);
        with_states(&surface, |states| prefer(states, scale));
    }
}

impl State {
    /// The scale of the monitor `surface` is on: a layer surface's output,
    /// or the one its window's middle is on; the primary's otherwise.
    fn scale_for(&self, surface: &WlSurface) -> f64 {
        let output = self
            .outputs
            .iter()
            .find(|o| {
                layer_map_for_output(o)
                    .layer_for_surface(surface, WindowSurfaceType::ALL)
                    .is_some()
            })
            .cloned()
            .or_else(|| self.window_for(surface).map(|w| self.output_of(&w)))
            .unwrap_or_else(|| self.output.clone());
        output.current_scale().fractional_scale()
    }

    /// Tells every surface its output's scale again, after it changed (the
    /// nested window moved to a monitor with another scale) or the surface
    /// moved to another monitor.
    pub fn update_preferred_scale(&self) {
        for window in self.space.elements() {
            let scale = self.output_of(window).current_scale().fractional_scale();
            window.with_surfaces(|_, states| prefer(states, scale));
        }
        for output in &self.outputs {
            for layer in layer_map_for_output(output).layers() {
                layer.with_surfaces(|_, states| {
                    prefer(states, output.current_scale().fractional_scale())
                });
            }
        }
    }
}

delegate_fractional_scale!(State);
delegate_viewporter!(State);
