//! Drags out of a piece's surface to other apps (ADR-0140): a [`Shell`](crate::Shell) hands the
//! host a [`Drag`] while the pointer is held down, and the host continues that press as a
//! Wayland drag, offering the types the drag lists and giving the bytes of one when the app
//! it's dropped on asks.
//!
//! sctk's data device wants its handler traits on the host, and a piece never takes drops, so
//! this opens a queue of its own on the host's connection, as Files does for its drags
//! (`files/src/dnd.rs`): a data device, and a thread that answers the sources' `send`s, so the
//! data goes out however idle the surface is, and tells the host when a drag is over
//! ([`Shell::drag_ended`](crate::Shell::drag_ended)). The queue is made when the first drag starts.

use std::io::Write;
use std::sync::Arc;

use smithay_client_toolkit::reexports::calloop::channel::Sender;

use smithay_client_toolkit::reexports::client::{
    Connection, Dispatch, Proxy, QueueHandle, event_created_child,
    globals::{GlobalListContents, registry_queue_init},
    protocol::{
        wl_data_device::{self, WlDataDevice},
        wl_data_device_manager::{DndAction, WlDataDeviceManager},
        wl_data_offer::WlDataOffer,
        wl_data_source::{self, WlDataSource},
        wl_registry::WlRegistry,
        wl_seat::WlSeat,
        wl_surface::WlSurface,
    },
};

/// Gives the bytes of one of a drag's types, on a thread of its own: `None` if there are none.
pub type Data = Arc<dyn Fn(&str) -> Option<Vec<u8>> + Send + Sync>;

/// A picture to show under the pointer while dragging: premultiplied ARGB pixels, `scale`
/// of them to the point. The pointer is `hotspot` pixels in from its corner.
pub struct Icon {
    pub size: (u32, u32),
    pub scale: i32,
    pub pixels: Vec<u32>,
    pub hotspot: (i32, i32),
}

/// What a shell drags.
pub struct Drag {
    /// The MIME types it offers, in the order the source prefers them.
    pub types: Vec<String>,
    pub data: Data,
    pub icon: Option<Icon>,
}

pub(crate) struct Dragger {
    conn: Connection,
    qh: QueueHandle<State>,
    manager: WlDataDeviceManager,
    device: WlDataDevice,
}

/// The thread's side: where to say a drag is over.
struct State {
    ended: Sender<()>,
}

impl Dragger {
    /// `None` if the compositor has no data device manager.
    pub fn new(conn: &Connection, seat: &WlSeat, ended: Sender<()>) -> Option<Self> {
        let (globals, mut queue) = registry_queue_init::<State>(conn).ok()?;
        let qh = queue.handle();
        let manager: WlDataDeviceManager = globals.bind(&qh, 3..=3, ()).ok()?;
        let device = manager.get_data_device(seat, &qh, ());
        let mut state = State { ended };
        queue.roundtrip(&mut state).ok()?;
        std::thread::Builder::new()
            .name("drag-out".into())
            .spawn(move || while queue.blocking_dispatch(&mut state).is_ok() {})
            .ok()?;
        Some(Self {
            conn: conn.clone(),
            qh,
            manager,
            device,
        })
    }

    /// Starts `drag` from `origin`, the surface the press `serial` was on, under `icon`.
    pub fn start(&self, drag: &Drag, origin: &WlSurface, icon: Option<&WlSurface>, serial: u32) {
        let source = self.manager.create_data_source(&self.qh, drag.data.clone());
        for mime in &drag.types {
            source.offer(mime.clone());
        }
        source.set_actions(DndAction::Copy);
        self.device.start_drag(Some(&source), origin, icon, serial);
        let _ = self.conn.flush();
    }
}

impl Dispatch<WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: <WlRegistry as Proxy>::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlDataDeviceManager, ()> for State {
    fn event(
        _: &mut Self,
        _: &WlDataDeviceManager,
        _: <WlDataDeviceManager as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlDataDevice, ()> for State {
    fn event(
        _: &mut Self,
        _: &WlDataDevice,
        event: wl_data_device::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // A piece takes no drops and no pastes: every offer is let go at once.
        match event {
            wl_data_device::Event::Selection { id: Some(offer) }
            | wl_data_device::Event::Enter {
                id: Some(offer), ..
            } => offer.destroy(),
            _ => {}
        }
    }

    event_created_child!(State, WlDataDevice, [
        wl_data_device::EVT_DATA_OFFER_OPCODE => (WlDataOffer, ()),
    ]);
}

impl Dispatch<WlDataOffer, ()> for State {
    fn event(
        _: &mut Self,
        _: &WlDataOffer,
        _: <WlDataOffer as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlDataSource, Data> for State {
    fn event(
        state: &mut Self,
        source: &WlDataSource,
        event: wl_data_source::Event,
        data: &Data,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_data_source::Event::Send { mime_type, fd } => {
                // On a thread: the bytes may come from a bus call, and the other app reads
                // at its own pace.
                let data = data.clone();
                let _ = std::thread::Builder::new()
                    .name("drag-send".into())
                    .spawn(move || {
                        if let Some(bytes) = data(&mime_type) {
                            let _ = std::fs::File::from(fd).write_all(&bytes);
                        }
                    });
            }
            wl_data_source::Event::Cancelled | wl_data_source::Event::DndFinished => {
                source.destroy();
                let _ = state.ended.send(());
            }
            _ => {}
        }
    }
}
