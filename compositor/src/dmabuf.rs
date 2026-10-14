//! linux-dmabuf, for either GPU backend: clients hand over GPU buffers, and
//! the GLES renderer checks it can read each one before saying yes.

use smithay::{
    backend::{
        allocator::dmabuf::Dmabuf,
        renderer::{ImportDma, gles::GlesRenderer},
    },
    delegate_dmabuf,
    wayland::dmabuf::{DmabufGlobal, DmabufHandler, DmabufState, ImportNotifier},
};

use crate::state::State;

impl State {
    /// The GLES renderer, when there's a GPU backend.
    pub fn gles_renderer(&mut self) -> Option<&mut GlesRenderer> {
        if let Some(w) = self.winit.as_mut() {
            return Some(w.backend.renderer());
        }
        self.udev.as_mut().map(|u| &mut u.renderer)
    }
}

impl DmabufHandler for State {
    fn dmabuf_state(&mut self) -> &mut DmabufState {
        if let Some(w) = self.winit.as_mut() {
            return &mut w.dmabuf_state;
        }
        &mut self
            .udev
            .as_mut()
            .expect("linux-dmabuf is only offered with a GPU backend")
            .dmabuf_state
    }

    fn dmabuf_imported(&mut self, _: &DmabufGlobal, dmabuf: Dmabuf, notifier: ImportNotifier) {
        let imported = self
            .gles_renderer()
            .is_some_and(|r| r.import_dmabuf(&dmabuf, None).is_ok());
        if imported {
            let _ = notifier.successful::<State>();
        } else {
            notifier.failed();
        }
    }
}

delegate_dmabuf!(State);
