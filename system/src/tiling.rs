//! The client side of the compositor's tiling on the session bus
//! (ADR-0049): `com.sectorflabs.Patina.Tiling`. Anyone may list and place
//! windows and watch drags; a tiler `claim`s the Tiler name, and from then
//! on the compositor leaves the decisions to it: it reports drags and
//! Super+arrows, and places windows where it's told, until the tiler leaves
//! the bus. `examples/thirds.rs` is a whole tiler.

use zbus::blocking::{Connection, Proxy};

const NAME: &str = "com.sectorflabs.Patina.Tiling";
const PATH: &str = "/com/sectorflabs/Patina/Tiling";
/// The name whose owner decides.
pub const TILER: &str = "com.sectorflabs.Patina.Tiler";

/// x, y, width and height, in the desktop's logical pixels.
pub type Rect = (i32, i32, i32, i32);

/// An open window.
#[derive(Debug, Clone, PartialEq)]
pub struct Window {
    pub id: u64,
    pub app_id: String,
    pub title: String,
    /// "", "maximized", "left", "right", or "rect" (placed by a tiler).
    pub placement: String,
    pub rect: Rect,
}

/// What the compositor reports.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    DragStarted(u64),
    /// The pointer, dragging a window, in logical pixels.
    DragMotion(u64, f64, f64),
    DragEnded(u64, f64, f64),
    /// Super with an arrow: "up", "down", "left" or "right"; only to a tiler.
    SnapRequested(u64, String),
    WorkAreaChanged(Rect),
}

/// The compositor's tiling, over `bus`.
pub struct Tiling<'a> {
    proxy: Proxy<'a>,
}

impl<'a> Tiling<'a> {
    pub fn new(bus: &'a Connection) -> zbus::Result<Self> {
        Ok(Self {
            proxy: Proxy::new(bus, NAME, PATH, NAME)?,
        })
    }

    /// The open windows, bottom to top.
    pub fn windows(&self) -> zbus::Result<Vec<Window>> {
        let rows: Vec<(u64, String, String, String, Rect)> = self.proxy.call("Windows", &())?;
        Ok(rows
            .into_iter()
            .map(|(id, app_id, title, placement, rect)| Window {
                id,
                app_id,
                title,
                placement,
                rect,
            })
            .collect())
    }

    /// Where windows tile: the screen less the bar.
    pub fn work_area(&self) -> zbus::Result<Rect> {
        self.proxy.call("WorkArea", &())
    }

    /// Tiles window `id` at `rect`; all of the work area maximises it.
    pub fn place(&self, id: u64, (x, y, w, h): Rect) -> zbus::Result<()> {
        self.proxy.call("Place", &(id, x, y, w, h))
    }

    /// Puts window `id` back as it was before it was tiled.
    pub fn restore(&self, id: u64) -> zbus::Result<()> {
        self.proxy.call("Restore", &(id,))
    }

    /// Shows where a drop would go; hidden again when the drag ends.
    pub fn show_preview(&self, (x, y, w, h): Rect) -> zbus::Result<()> {
        self.proxy.call("ShowPreview", &(x, y, w, h))
    }

    pub fn hide_preview(&self) -> zbus::Result<()> {
        self.proxy.call("HidePreview", &())
    }

    /// What the compositor reports, as it happens.
    pub fn events(&self) -> zbus::Result<impl Iterator<Item = Event> + use<'a>> {
        Ok(self
            .proxy
            .receive_all_signals()?
            .filter_map(|message| event(&message)))
    }
}

fn event(message: &zbus::Message) -> Option<Event> {
    let header = message.header();
    let body = message.body();
    Some(match header.member()?.as_str() {
        "DragStarted" => Event::DragStarted(body.deserialize().ok()?),
        "DragMotion" => {
            let (id, x, y) = body.deserialize().ok()?;
            Event::DragMotion(id, x, y)
        }
        "DragEnded" => {
            let (id, x, y) = body.deserialize().ok()?;
            Event::DragEnded(id, x, y)
        }
        "SnapRequested" => {
            let (id, direction) = body.deserialize().ok()?;
            Event::SnapRequested(id, direction)
        }
        "WorkAreaChanged" => Event::WorkAreaChanged(body.deserialize().ok()?),
        _ => return None,
    })
}

/// Makes the caller the tiler, taking over from one already deciding.
/// It decides until its connection closes.
pub fn claim(bus: &Connection) -> zbus::Result<()> {
    use zbus::fdo::{RequestNameFlags, RequestNameReply};
    let flags = RequestNameFlags::ReplaceExisting | RequestNameFlags::DoNotQueue;
    match bus.request_name_with_flags(TILER, flags)? {
        RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner => Ok(()),
        _ => Err(zbus::Error::Failure(
            "another tiler holds the name and won't let go".into(),
        )),
    }
}
