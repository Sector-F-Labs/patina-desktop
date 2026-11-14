//! The compositor's clipboard history (ADR-0114), through its
//! `com.sectorflabs.Patina.Clipboard` on the session bus: the copies it
//! keeps, picking one, deleting one, clearing them, pausing the
//! history, and the content of an entry for a drag (ADR-0140). For the bar's tray icon and its applet, patina-clipboard.

use std::path::PathBuf;

use zbus::blocking::{Connection, Proxy};

const NAME: &str = "com.sectorflabs.Patina.Clipboard";
const PATH: &str = "/com/sectorflabs/Patina/Clipboard";

/// What a copy holds, as the compositor describes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Text,
    Image,
    /// Anything else the compositor keeps: shown by its preview.
    Other,
}

impl Kind {
    fn parse(word: &str) -> Self {
        match word {
            "text" => Self::Text,
            "image" => Self::Image,
            _ => Self::Other,
        }
    }
}

/// One copy in the history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: u64,
    pub kind: Kind,
    /// A text's first line, cut short, or a description of an image.
    pub preview: String,
    /// A small image of it, a file the applet loads; images only.
    pub thumbnail: Option<PathBuf>,
    /// When it was copied, in seconds since the epoch.
    pub at: u32,
}

/// The history, newest first, and its switches.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Status {
    pub entries: Vec<Entry>,
    /// Off: nothing is kept.
    pub enabled: bool,
    /// Paused: what's kept stays, and nothing new is added.
    pub paused: bool,
}

/// The compositor's rows, `(id, kind, preview, thumbnail path, time)`, as entries.
fn entries(rows: Vec<(u64, String, String, String, u32)>) -> Vec<Entry> {
    rows.into_iter()
        .map(|(id, kind, preview, thumbnail, at)| Entry {
            id,
            kind: Kind::parse(&kind),
            preview,
            thumbnail: (!thumbnail.is_empty()).then(|| PathBuf::from(thumbnail)),
            at,
        })
        .collect()
}

fn session() -> Result<Connection, String> {
    Connection::session().map_err(|e| format!("no session bus: {e}"))
}

fn proxy(bus: &Connection) -> Result<Proxy<'_>, String> {
    Proxy::new(bus, NAME, PATH, NAME).map_err(|e| e.to_string())
}

/// Whether the compositor serves the history here: not under another desktop.
fn served(bus: &Connection) -> Result<bool, String> {
    let dbus = zbus::blocking::fdo::DBusProxy::new(bus).map_err(|e| e.to_string())?;
    let name = zbus::names::BusName::try_from(NAME).map_err(|e| e.to_string())?;
    dbus.name_has_owner(name).map_err(|e| e.to_string())
}

fn read(p: &Proxy<'_>) -> Result<Status, String> {
    let rows: Vec<(u64, String, String, String, u32)> =
        p.call("List", &()).map_err(|e| e.to_string())?;
    Ok(Status {
        entries: entries(rows),
        enabled: p.get_property("Enabled").map_err(|e| e.to_string())?,
        paused: p.get_property("Paused").map_err(|e| e.to_string())?,
    })
}

/// The history now; `None` when nothing serves it.
pub fn status() -> Result<Option<Status>, String> {
    let bus = session()?;
    if !served(&bus)? {
        return Ok(None);
    }
    read(&proxy(&bus)?).map(Some)
}

/// Makes entry `id` the clipboard, and the newest.
pub fn pick(id: u64) -> Result<(), String> {
    with(|p| p.call::<_, _, ()>("Pick", &(id,)))
}

/// The MIME types entry `id` holds, to say what a drag of it offers.
pub fn types(id: u64) -> Result<Vec<String>, String> {
    let bus = session()?;
    proxy(&bus)?
        .call("Types", &(id,))
        .map_err(|e| e.to_string())
}

/// Entry `id`'s bytes as `mime`, for a drag to give the app it's dropped on.
pub fn content(id: u64, mime: &str) -> Result<Vec<u8>, String> {
    let bus = session()?;
    proxy(&bus)?
        .call("Content", &(id, mime))
        .map_err(|e| e.to_string())
}

/// Removes entry `id`.
pub fn delete(id: u64) -> Result<(), String> {
    with(|p| p.call::<_, _, ()>("Delete", &(id,)))
}

/// Removes every entry.
pub fn clear() -> Result<(), String> {
    with(|p| p.call::<_, _, ()>("Clear", &()))
}

/// Pauses the history, or lets it go on.
pub fn set_paused(paused: bool) -> Result<(), String> {
    let bus = session()?;
    proxy(&bus)?
        .set_property("Paused", paused)
        .map_err(|e| e.to_string())
}

fn with(f: impl FnOnce(&Proxy<'_>) -> zbus::Result<()>) -> Result<(), String> {
    let bus = session()?;
    f(&proxy(&bus)?).map_err(|e| e.to_string())
}

/// Gives `send` the history now and each time it changes, on this thread, until it returns
/// false. While nothing serves it, `send` is given `None`, and this asks again every few
/// seconds.
pub fn watch(mut send: impl FnMut(Option<Status>) -> bool) {
    loop {
        if let Ok(bus) = session()
            && served(&bus).unwrap_or(false)
            && let Ok(p) = proxy(&bus)
            && let Ok(changes) = p.receive_signal("Changed")
        {
            if !send(read(&p).ok()) {
                return;
            }
            for _ in changes {
                if !send(read(&p).ok()) {
                    return;
                }
            }
        }
        if !send(None) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_secs(5));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_become_entries() {
        let rows = vec![
            (
                3,
                "image".into(),
                "Image (png, 1 KB)".into(),
                "/run/t/3.png".into(),
                10,
            ),
            (2, "text".into(), "hello".into(), String::new(), 9),
            (1, "files".into(), "a".into(), String::new(), 8),
        ];
        let list = entries(rows);
        assert_eq!(list[0].kind, Kind::Image);
        assert_eq!(list[0].thumbnail, Some(PathBuf::from("/run/t/3.png")));
        assert_eq!(list[1].kind, Kind::Text);
        assert_eq!(list[1].thumbnail, None);
        assert_eq!(list[2].kind, Kind::Other);
    }
}
