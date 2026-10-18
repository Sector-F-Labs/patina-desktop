//! The client side of the compositor's tiling on the session bus
//! (ADR-0049, ADR-0053): `com.sectorflabs.Patina.Tiling`. Anyone may list
//! and place windows and watch drags. The compositor decides nothing: it
//! reports drags and Super+arrows, and places windows where it's told.
//! patina-tiler decides, holding the Tiler name with `claim_default`; a
//! tiler that `claim`s it takes over until it leaves the bus, and then
//! patina-tiler decides again. `examples/thirds.rs` is a whole tiler.

use zbus::blocking::{Connection, Proxy};

/// The compositor's name on the bus.
pub const NAME: &str = "com.sectorflabs.Patina.Tiling";
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
    /// "", "maximized" (by the app), "fill" (all of the work area) or
    /// "rect" (placed by a tiler).
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
    /// A tiled window's new rectangle, after the line between it and its
    /// neighbour was dragged.
    Resized(u64, Rect),
    /// A window first showing, at the app's own size: its placement (as in
    /// `Window`) and rectangle.
    WindowOpened {
        id: u64,
        app_id: String,
        placement: String,
        rect: Rect,
    },
    /// A window closing, and where it floated, or did before it was placed
    /// or went fullscreen.
    WindowClosed {
        id: u64,
        app_id: String,
        rect: Rect,
    },
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

    /// Where windows tile: the screen less the bar, and less the gaps
    /// around it when they're on.
    pub fn work_area(&self) -> zbus::Result<Rect> {
        self.proxy.call("WorkArea", &())
    }

    /// The gap to leave between tiled windows; 0 when gaps are off.
    pub fn gap(&self) -> zbus::Result<i32> {
        self.proxy.call("Gap", &())
    }

    /// The whole screen, bar and all, for where the pointer meets its
    /// edges.
    pub fn screen(&self) -> zbus::Result<Rect> {
        self.proxy.call("Screen", &())
    }

    /// Tiles window `id` at `rect`; all of the work area fills it, tiled,
    /// not maximised.
    pub fn place(&self, id: u64, (x, y, w, h): Rect) -> zbus::Result<()> {
        self.proxy.call("Place", &(id, x, y, w, h))
    }

    /// Moves floating window `id` so its top left is at `at`.
    pub fn move_to(&self, id: u64, (x, y): (i32, i32)) -> zbus::Result<()> {
        self.proxy.call("Move", &(id, x, y))
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
        "Resized" => {
            let (id, x, y, w, h) = body.deserialize().ok()?;
            Event::Resized(id, (x, y, w, h))
        }
        "WindowOpened" => {
            let (id, app_id, placement, x, y, w, h) = body.deserialize().ok()?;
            Event::WindowOpened {
                id,
                app_id,
                placement,
                rect: (x, y, w, h),
            }
        }
        "WindowClosed" => {
            let (id, app_id, x, y, w, h) = body.deserialize().ok()?;
            Event::WindowClosed {
                id,
                app_id,
                rect: (x, y, w, h),
            }
        }
        _ => return None,
    })
}

/// Makes the caller the default tiler: it decides now, or as soon as the
/// name is free, and gives way to a tiler that `claim`s it, deciding
/// again once that one leaves. It takes over from an older default tiler,
/// one left running by a compositor gone, which would otherwise hold the
/// name for good; never from a `claim`ed one, which doesn't allow it. Ask
/// `deciding` before acting.
pub fn claim_default(bus: &Connection) -> zbus::Result<()> {
    use zbus::fdo::RequestNameFlags;
    let flags = RequestNameFlags::AllowReplacement | RequestNameFlags::ReplaceExisting;
    bus.request_name_with_flags(TILER, flags)?;
    Ok(())
}

/// Whether the caller owns the Tiler name now.
pub fn deciding(bus: &Connection) -> bool {
    let Ok(dbus) = zbus::blocking::fdo::DBusProxy::new(bus) else {
        return false;
    };
    let Ok(name) = zbus::names::BusName::try_from(TILER) else {
        return false;
    };
    match (dbus.get_name_owner(name), bus.unique_name()) {
        (Ok(owner), Some(me)) => owner.as_str() == me.as_str(),
        _ => false,
    }
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

/// On a bus of its own: `dbus-run-session -- cargo test -p patina-system
/// tiling -- --ignored`.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "needs a session bus of its own"]
    fn another_tiler_takes_over_and_the_default_comes_back() {
        let stale = Connection::session().unwrap();
        claim_default(&stale).unwrap();
        assert!(deciding(&stale));
        // A newer default takes over from one left behind.
        let default = Connection::session().unwrap();
        claim_default(&default).unwrap();
        assert!(deciding(&default));
        assert!(!deciding(&stale));
        drop(stale);

        let other = Connection::session().unwrap();
        claim(&other).unwrap();
        assert!(deciding(&other));
        assert!(!deciding(&default), "the default gave way");
        // A default starting while it decides doesn't take over.
        let restarted = Connection::session().unwrap();
        claim_default(&restarted).unwrap();
        assert!(deciding(&other));
        drop(restarted);

        drop(other);
        let back = std::time::Instant::now();
        while !deciding(&default) {
            assert!(back.elapsed().as_secs() < 5, "the default never came back");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
}
