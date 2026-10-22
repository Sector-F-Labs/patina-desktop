//! Apps pinned to the bar (ADR-0080): `pinned` in the bar's config, a list
//! of app ids, and the order of the bar's tabs that follows from it. The
//! bar draws its tabs in that order, and the compositor's Super+1 to 9 and
//! Alt+Tab follow it too, each working it out from the same file and the
//! same windows: each pinned app in its place, as its windows' tabs or, with
//! none open, a tab that starts it; then the other windows.

use std::path::Path;

use toml_edit::{Array, DocumentMut, value};

/// The bar config's key.
pub const KEY: &str = "pinned";

/// The apps pinned in the bar's config `text`, in order, each once.
pub fn read(text: &str) -> Vec<String> {
    let Ok(doc) = text.parse::<DocumentMut>() else {
        return Vec::new();
    };
    let mut pins: Vec<String> = Vec::new();
    for pin in doc
        .get(KEY)
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str())
    {
        if !pin.is_empty() && !pins.iter().any(|p| p == pin) {
            pins.push(pin.to_string());
        }
    }
    pins
}

/// The bar config `text` with `pins` as its pinned apps, the rest as it
/// was.
pub fn with_pins(text: &str, pins: &[String]) -> Result<String, String> {
    let mut doc = text.parse::<DocumentMut>().map_err(|e| e.to_string())?;
    let list: Array = pins.iter().map(String::as_str).collect();
    doc[KEY] = value(list);
    Ok(doc.to_string())
}

/// Writes `pins` into the bar's config at `path`, which needn't exist yet.
pub fn set(path: &Path, pins: &[String]) -> Result<(), String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let text = with_pins(&text, pins).map_err(|e| format!("{}: {e}", path.display()))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
}

/// The compositor's tab order on the bus (`com.sectorflabs.Patina.Tabs`).
pub const NAME: &str = "com.sectorflabs.Patina.Tabs";
const PATH: &str = "/com/sectorflabs/Patina/Tabs";

/// Asks the compositor to move the window at `from` in its list of open
/// windows (the order wlr-foreign-toplevel announced them in) to `to`, as
/// `Vec::remove` then `insert` would. Its `Moved` signal follows. Blocks
/// for the answer.
pub fn move_window(from: usize, to: usize) -> Result<(), String> {
    let bus = zbus::blocking::Connection::session().map_err(|e| e.to_string())?;
    let proxy = zbus::blocking::Proxy::new(&bus, NAME, PATH, NAME).map_err(|e| e.to_string())?;
    proxy
        .call::<_, _, ()>("Move", &(from as u32, to as u32))
        .map_err(|e| e.to_string())
}

/// Calls `send` with each window the compositor moves, from and to, until
/// it returns false; without a session bus, tries again every few seconds.
pub fn watch_moves(mut send: impl FnMut((usize, usize)) -> bool) {
    loop {
        let signals = zbus::blocking::Connection::session().and_then(|bus| {
            zbus::blocking::Proxy::new_owned(bus, NAME, PATH, NAME)?.receive_signal("Moved")
        });
        if let Ok(signals) = signals {
            for signal in signals {
                if let Ok((from, to)) = signal.body().deserialize::<(u32, u32)>()
                    && !send((from as usize, to as usize))
                {
                    return;
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_secs(5));
    }
}

/// One of the bar's tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    /// The window at this index in the list of windows.
    Window(usize),
    /// The pinned app at this index, which has no window open.
    Launcher(usize),
}

/// The bar's tabs, for windows with these app ids, in the compositor's
/// order: each pinned app's windows where it's pinned, in their order, or
/// its launcher if it has none; then the windows of apps not pinned.
pub fn arrange<S: AsRef<str>>(pinned: &[String], app_ids: &[S]) -> Vec<Tab> {
    let pin_of = |i: usize| pinned.iter().position(|p| p == app_ids[i].as_ref());
    let mut tabs = Vec::with_capacity(pinned.len() + app_ids.len());
    for (p, _) in pinned.iter().enumerate() {
        let before = tabs.len();
        tabs.extend(
            (0..app_ids.len())
                .filter(|&i| pin_of(i) == Some(p))
                .map(Tab::Window),
        );
        if tabs.len() == before {
            tabs.push(Tab::Launcher(p));
        }
    }
    tabs.extend(
        (0..app_ids.len())
            .filter(|&i| pin_of(i).is_none())
            .map(Tab::Window),
    );
    tabs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pins(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    #[test]
    fn pins_are_read_in_order_once() {
        assert_eq!(
            read("skin = \"x\"\npinned = [\"firefox\", \"foot\", \"firefox\", \"\"]\n"),
            pins(&["firefox", "foot"])
        );
        assert!(read("").is_empty());
        assert!(read("pinned = \"firefox\"").is_empty(), "not a list");
        assert!(read("pinned = [").is_empty(), "a broken file");
    }

    #[test]
    fn pins_are_written_beside_the_rest() {
        let text = "# the bar\nskin = \"Calendar\"\n\n[gaps]\nx = 1\n";
        let written = with_pins(text, &pins(&["firefox", "foot"])).unwrap();
        assert!(
            written.starts_with("# the bar\nskin = \"Calendar\"\n"),
            "{written}"
        );
        assert_eq!(read(&written), pins(&["firefox", "foot"]));
        assert!(written.contains("[gaps]\nx = 1"), "{written}");
        let fewer = with_pins(&written, &pins(&["foot"])).unwrap();
        assert_eq!(read(&fewer), pins(&["foot"]));
        assert_eq!(fewer.matches("pinned").count(), 1, "{fewer}");
    }

    #[test]
    fn pinned_apps_come_first_with_their_windows() {
        use Tab::*;
        let open = ["foot", "firefox", "nautilus", "foot"];
        assert_eq!(
            arrange(&pins(&["firefox", "steam", "foot"]), &open),
            [Window(1), Launcher(1), Window(0), Window(3), Window(2)]
        );
        assert_eq!(
            arrange(&[], &open),
            [Window(0), Window(1), Window(2), Window(3)]
        );
        assert_eq!(arrange::<&str>(&pins(&["steam"]), &[]), [Launcher(0)]);
    }
}
