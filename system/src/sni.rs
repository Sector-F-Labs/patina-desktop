//! The system tray (ADR-0064): apps' status icons (Steam, chat apps,
//! Nextcloud), by the freedesktop StatusNotifierItem protocol on the
//! session bus, as KDE and libappindicator speak it.
//!
//! [`start`] serves `org.kde.StatusNotifierWatcher`, which apps register
//! their items with (or, if another program already does, listens to
//! that one), follows each item's properties and change signals, and sends
//! the bar every change as the whole list. [`Tray`] asks an item to
//! activate, and reads and clicks its menu, which apps serve by
//! `com.canonical.dbusmenu`. All of it runs on threads of its own.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, mpsc};

use zbus::blocking::{Connection, Proxy};
use zbus::proxy::CacheProperties;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

pub const WATCHER: &str = "org.kde.StatusNotifierWatcher";
const WATCHER_PATH: &str = "/StatusNotifierWatcher";
const ITEM: &str = "org.kde.StatusNotifierItem";
const DEFAULT_ITEM_PATH: &str = "/StatusNotifierItem";
const MENU: &str = "com.canonical.dbusmenu";

/// A dbusmenu node as GetLayout returns it: id, properties, children.
type Layout = (i32, HashMap<String, OwnedValue>, Vec<OwnedValue>);

/// What an item shows.
#[derive(Debug, Clone, PartialEq)]
pub enum Icon {
    /// `size` by `size` straight (not premultiplied) RGBA, from the item's
    /// own pixels, scaled to the size asked for.
    Pixels { size: u32, rgba: Vec<u8> },
    /// An icon by name, in the icon theme, or in `theme_path` first.
    Named {
        name: String,
        theme_path: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Status {
    /// Nothing worth showing now: hidden.
    Passive,
    #[default]
    Active,
    NeedsAttention,
}

/// An app's status icon.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    /// Its bus name and object path, as `name/path`.
    pub key: String,
    pub id: String,
    pub title: String,
    pub status: Status,
    pub icon: Option<Icon>,
    pub tooltip: String,
    /// It only has a menu: a click shows it, rather than activating.
    pub item_is_menu: bool,
    pub has_menu: bool,
}

/// An entry of an item's menu.
#[derive(Debug, Clone, PartialEq)]
pub enum MenuEntry {
    Separator,
    Entry {
        id: i32,
        label: String,
        enabled: bool,
        /// A checkmark or radio button's state, if it has one.
        checked: Option<bool>,
        children: Vec<MenuEntry>,
    },
}

/// The key of an item registered as `service` by the bus client `sender`:
/// `service` is the item's bus name, or its object path on the sender's
/// connection (as libappindicator registers).
pub fn item_key(service: &str, sender: &str) -> String {
    if service.starts_with('/') {
        format!("{sender}{service}")
    } else if service.contains('/') {
        service.to_string()
    } else {
        format!("{service}{DEFAULT_ITEM_PATH}")
    }
}

/// `name/path` as its bus name and object path.
fn split_key(key: &str) -> (&str, &str) {
    match key.find('/') {
        Some(i) => (&key[..i], &key[i..]),
        None => (key, DEFAULT_ITEM_PATH),
    }
}

/// A menu label without its mnemonic underscores: `_Library` is
/// "Library", and `__` an underscore.
pub fn label(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '_' {
            if chars.peek() == Some(&'_') {
                out.push('_');
                chars.next();
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// An item's pixels as an icon `size` big: the smallest at least that
/// big (or the largest), ARGB in network order made straight RGBA, and
/// scaled to `size`.
pub fn pixels(pixmaps: &[(i32, i32, Vec<u8>)], size: u32) -> Option<Icon> {
    let usable = pixmaps
        .iter()
        .filter(|(w, h, data)| *w > 0 && *h > 0 && data.len() >= (*w * *h * 4) as usize);
    let best = usable
        .clone()
        .filter(|(w, h, _)| (*w).min(*h) as u32 >= size)
        .min_by_key(|(w, _, _)| *w)
        .or_else(|| usable.max_by_key(|(w, _, _)| *w))?;
    let (w, h, data) = (best.0 as u32, best.1 as u32, &best.2);
    let rgba: Vec<u8> = data[..(w * h * 4) as usize]
        .chunks_exact(4)
        .flat_map(|p| [p[1], p[2], p[3], p[0]])
        .collect();
    let image = image::RgbaImage::from_raw(w, h, rgba)?;
    let scaled = if (w, h) == (size, size) {
        image
    } else {
        image::imageops::resize(&image, size, size, image::imageops::FilterType::Lanczos3)
    };
    Some(Icon::Pixels {
        size,
        rgba: scaled.into_raw(),
    })
}

/// A menu node from dbusmenu's layout: an `(ia{sv}av)` structure.
fn menu_entry(node: &Value) -> Option<MenuEntry> {
    let node = match node {
        Value::Value(inner) => inner.as_ref(),
        other => other,
    };
    let Value::Structure(structure) = node else {
        return None;
    };
    let fields = structure.fields();
    let id = match fields.first()? {
        Value::I32(id) => *id,
        _ => return None,
    };
    let mut props: HashMap<String, Value> = HashMap::new();
    if let Some(Value::Dict(dict)) = fields.get(1) {
        for (k, v) in dict.iter() {
            if let (Value::Str(k), v) = (k, v) {
                let v = match v {
                    Value::Value(inner) => inner.as_ref(),
                    other => other,
                };
                props.insert(k.to_string(), v.try_clone().ok()?);
            }
        }
    }
    let text = |key: &str| match props.get(key) {
        Some(Value::Str(s)) => Some(s.to_string()),
        _ => None,
    };
    let flag = |key: &str, default: bool| match props.get(key) {
        Some(Value::Bool(b)) => *b,
        _ => default,
    };
    if !flag("visible", true) {
        return None;
    }
    if text("type").as_deref() == Some("separator") {
        return Some(MenuEntry::Separator);
    }
    let checked = match (text("toggle-type").as_deref(), props.get("toggle-state")) {
        (Some("checkmark" | "radio"), Some(Value::I32(state))) => Some(*state == 1),
        (Some("checkmark" | "radio"), _) => Some(false),
        _ => None,
    };
    let children = match fields.get(2) {
        Some(Value::Array(children)) => children.iter().filter_map(menu_entry).collect(),
        _ => Vec::new(),
    };
    Some(MenuEntry::Entry {
        id,
        label: label(&text("label").unwrap_or_default()),
        enabled: flag("enabled", true),
        checked,
        children,
    })
}

/// A request to the tray's thread.
enum Work {
    Registered(String),
    Unregistered(String),
    Changed(String),
    Activate(String, i32, i32),
    SecondaryActivate(String, i32, i32),
    /// The item's menu, read and given to the callback; or, without one,
    /// asked to show its own.
    Menu(String, i32, i32, Box<dyn FnOnce(Vec<MenuEntry>) + Send>),
    Clicked(String, i32),
}

/// The bar's handle on the tray's thread.
#[derive(Clone)]
pub struct Tray {
    work: mpsc::Sender<Work>,
}

impl Tray {
    /// A left click on item `key`, at (`x`, `y`) on the screen.
    pub fn activate(&self, key: &str, x: i32, y: i32) {
        let _ = self.work.send(Work::Activate(key.to_string(), x, y));
    }

    /// A middle click.
    pub fn secondary_activate(&self, key: &str, x: i32, y: i32) {
        let _ = self
            .work
            .send(Work::SecondaryActivate(key.to_string(), x, y));
    }

    /// Reads item `key`'s menu and gives it to `then`, on the tray's thread.
    /// An item without a dbusmenu is asked to show its own menu at (`x`,
    /// `y`) instead, and `then` gets nothing.
    pub fn menu(
        &self,
        key: &str,
        x: i32,
        y: i32,
        then: impl FnOnce(Vec<MenuEntry>) + Send + 'static,
    ) {
        let _ = self
            .work
            .send(Work::Menu(key.to_string(), x, y, Box::new(then)));
    }

    /// Menu entry `id` of item `key` was chosen.
    pub fn clicked(&self, key: &str, id: i32) {
        let _ = self.work.send(Work::Clicked(key.to_string(), id));
    }
}

/// The watcher apps register with, served on the bus.
struct Watcher {
    items: Arc<Mutex<Vec<String>>>,
    work: mpsc::Sender<Work>,
}

#[zbus::interface(name = "org.kde.StatusNotifierWatcher")]
impl Watcher {
    async fn register_status_notifier_item(
        &self,
        service: &str,
        #[zbus(header)] header: zbus::message::Header<'_>,
        #[zbus(signal_emitter)] emitter: zbus::object_server::SignalEmitter<'_>,
    ) {
        let sender = header.sender().map(|s| s.to_string()).unwrap_or_default();
        let key = item_key(service, &sender);
        {
            let mut items = self.items.lock().unwrap_or_else(|e| e.into_inner());
            if items.contains(&key) {
                return;
            }
            items.push(key.clone());
        }
        let _ = Self::status_notifier_item_registered(&emitter, &key).await;
        let _ = self.work.send(Work::Registered(key));
    }

    async fn register_status_notifier_host(&self, _service: &str) {}

    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> Vec<String> {
        self.items.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn protocol_version(&self) -> i32 {
        0
    }

    #[zbus(signal)]
    async fn status_notifier_item_registered(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        item: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn status_notifier_item_unregistered(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        item: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn status_notifier_host_registered(
        emitter: &zbus::object_server::SignalEmitter<'_>,
    ) -> zbus::Result<()>;
}

/// Starts the tray: the watcher (or listening to another program's), and
/// a thread following the items and doing what the bar asks, which sends
/// the items, each as `icon_size` pixels where they're pixels, to `send`
/// whenever anything changes, until `send` returns false.
pub fn start(
    icon_size: u32,
    send: impl Fn(Vec<Item>) -> bool + Send + 'static,
) -> Result<Tray, String> {
    let bus = Connection::session().map_err(|e| e.to_string())?;
    let (work, worked) = mpsc::channel::<Work>();
    let items = Arc::new(Mutex::new(Vec::new()));
    let watcher = Watcher {
        items: items.clone(),
        work: work.clone(),
    };
    let serving = bus.object_server().at(WATCHER_PATH, watcher).and_then(|_| {
        use zbus::fdo::RequestNameFlags;
        bus.request_name_with_flags(WATCHER, RequestNameFlags::DoNotQueue.into())
    });
    match serving {
        Ok(zbus::fdo::RequestNameReply::PrimaryOwner) => {
            watch_owners(&bus, items, work.clone())?;
        }
        _ => {
            // Another tray serves it: listen to that one.
            let _ = bus.object_server().remove::<Watcher, _>(WATCHER_PATH);
            follow_watcher(&bus, work.clone())?;
        }
    }
    let worker_bus = bus.clone();
    let follower = work.clone();
    std::thread::Builder::new()
        .name("tray".into())
        .spawn(move || run(&worker_bus, worked, follower, icon_size, send))
        .map_err(|e| e.to_string())?;
    Ok(Tray { work })
}

/// Drops items whose program left the bus.
fn watch_owners(
    bus: &Connection,
    items: Arc<Mutex<Vec<String>>>,
    work: mpsc::Sender<Work>,
) -> Result<(), String> {
    let changes = zbus::blocking::fdo::DBusProxy::new(bus)
        .and_then(|d| d.receive_name_owner_changed())
        .map_err(|e| e.to_string())?;
    let bus = bus.clone();
    std::thread::Builder::new()
        .name("tray-owners".into())
        .spawn(move || {
            for change in changes {
                let Ok(args) = change.args() else { continue };
                if args.new_owner().is_some() {
                    continue;
                }
                let name = args.name().to_string();
                let gone: Vec<String> = {
                    let mut items = items.lock().unwrap_or_else(|e| e.into_inner());
                    let gone = items
                        .iter()
                        .filter(|k| split_key(k).0 == name)
                        .cloned()
                        .collect();
                    items.retain(|k| split_key(k).0 != name);
                    gone
                };
                for key in gone {
                    if let Ok(iface) = bus.object_server().interface::<_, Watcher>(WATCHER_PATH) {
                        let emitter = iface.signal_emitter();
                        let _ = zbus::block_on(Watcher::status_notifier_item_unregistered(
                            emitter, &key,
                        ));
                    }
                    if work.send(Work::Unregistered(key)).is_err() {
                        return;
                    }
                }
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Follows another program's watcher: its items now, and as they come and
/// go.
fn follow_watcher(bus: &Connection, work: mpsc::Sender<Work>) -> Result<(), String> {
    let proxy = Proxy::new(bus, WATCHER, WATCHER_PATH, WATCHER).map_err(|e| e.to_string())?;
    let _ = proxy.call_method(
        "RegisterStatusNotifierHost",
        &(bus.unique_name().map(|n| n.to_string()).unwrap_or_default(),),
    );
    let signals = proxy.receive_all_signals().map_err(|e| e.to_string())?;
    for key in proxy
        .get_property::<Vec<String>>("RegisteredStatusNotifierItems")
        .unwrap_or_default()
    {
        let _ = work.send(Work::Registered(key));
    }
    std::thread::Builder::new()
        .name("tray-watcher".into())
        .spawn(move || {
            for signal in signals {
                let header = signal.header();
                let Some(member) = header.member() else {
                    continue;
                };
                let Ok(key) = signal.body().deserialize::<String>() else {
                    continue;
                };
                let sent = match member.as_str() {
                    "StatusNotifierItemRegistered" => work.send(Work::Registered(key)),
                    "StatusNotifierItemUnregistered" => work.send(Work::Unregistered(key)),
                    _ => Ok(()),
                };
                if sent.is_err() {
                    return;
                }
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// A proxy to item `key`'s interface `interface` at `path` (its own path
/// when `None`), reading properties afresh each time.
fn proxy<'a>(
    bus: &Connection,
    key: &str,
    path: Option<&str>,
    interface: &'a str,
) -> Option<Proxy<'a>> {
    let (name, item_path) = split_key(key);
    zbus::blocking::proxy::Builder::new(bus)
        .destination(name.to_string())
        .ok()?
        .path(path.unwrap_or(item_path).to_string())
        .ok()?
        .interface(interface)
        .ok()?
        .cache_properties(CacheProperties::No)
        .build()
        .ok()
}

/// Item `key`'s properties, now.
fn read_item(
    bus: &Connection,
    key: &str,
    icon_size: u32,
) -> Option<(Item, Option<OwnedObjectPath>)> {
    let p = proxy(bus, key, None, ITEM)?;
    let text = |name: &str| p.get_property::<String>(name).unwrap_or_default();
    let status = match text("Status").as_str() {
        "Passive" => Status::Passive,
        "NeedsAttention" => Status::NeedsAttention,
        _ => Status::Active,
    };
    let (name_key, pixmap_key) = if status == Status::NeedsAttention {
        ("AttentionIconName", "AttentionIconPixmap")
    } else {
        ("IconName", "IconPixmap")
    };
    let pixmaps = |key: &str| {
        p.get_property::<Vec<(i32, i32, Vec<u8>)>>(key)
            .unwrap_or_default()
    };
    let named = |name: String| {
        (!name.is_empty()).then(|| Icon::Named {
            name,
            theme_path: Some(text("IconThemePath")).filter(|t| !t.is_empty()),
        })
    };
    let icon = named(text(name_key))
        .or_else(|| pixels(&pixmaps(pixmap_key), icon_size))
        .or_else(|| named(text("IconName")))
        .or_else(|| pixels(&pixmaps("IconPixmap"), icon_size));
    let tooltip = p
        .get_property::<(String, Vec<(i32, i32, Vec<u8>)>, String, String)>("ToolTip")
        .map(|(_, _, title, _)| title)
        .unwrap_or_default();
    let menu = p
        .get_property::<OwnedObjectPath>("Menu")
        .ok()
        .filter(|path| path.as_str() != "/");
    let item = Item {
        key: key.to_string(),
        id: text("Id"),
        title: text("Title"),
        status,
        icon,
        tooltip,
        item_is_menu: p.get_property::<bool>("ItemIsMenu").unwrap_or(false),
        has_menu: menu.is_some(),
    };
    Some((item, menu))
}

/// Listens to item `key`'s change signals, asking for it to be read again.
fn follow_item(bus: &Connection, key: &str, work: mpsc::Sender<Work>) {
    let Some(p) = proxy(bus, key, None, ITEM) else {
        return;
    };
    let Ok(signals) = p.receive_all_signals() else {
        return;
    };
    let key = key.to_string();
    let _ = std::thread::Builder::new()
        .name("tray-item".into())
        .spawn(move || {
            for signal in signals {
                let header = signal.header();
                if header
                    .member()
                    .is_some_and(|m| m.as_str().starts_with("New"))
                    && work.send(Work::Changed(key.clone())).is_err()
                {
                    return;
                }
            }
        });
}

/// Item `key`'s menu, from its dbusmenu at `path`.
fn read_menu(bus: &Connection, key: &str, path: &str) -> Option<Vec<MenuEntry>> {
    let p = proxy(bus, key, Some(path), MENU)?;
    let _ = p.call_method("AboutToShow", &(0i32,));
    let reply = p
        .call_method("GetLayout", &(0i32, -1i32, Vec::<String>::new()))
        .ok()?;
    let (_revision, (_id, _props, children)): (u32, Layout) = reply.body().deserialize().ok()?;
    Some(children.iter().filter_map(|c| menu_entry(c)).collect())
}

fn run(
    bus: &Connection,
    worked: mpsc::Receiver<Work>,
    work: mpsc::Sender<Work>,
    icon_size: u32,
    send: impl Fn(Vec<Item>) -> bool,
) {
    let mut items: Vec<Item> = Vec::new();
    let mut menus: HashMap<String, OwnedObjectPath> = HashMap::new();
    for job in worked {
        let changed = match job {
            Work::Registered(key) => {
                if items.iter().any(|i| i.key == key) {
                    false
                } else if let Some((item, menu)) = read_item(bus, &key, icon_size) {
                    follow_item(bus, &key, work.clone());
                    if let Some(menu) = menu {
                        menus.insert(key.clone(), menu);
                    }
                    items.push(item);
                    true
                } else {
                    false
                }
            }
            Work::Unregistered(key) => {
                menus.remove(&key);
                let before = items.len();
                items.retain(|i| i.key != key);
                items.len() != before
            }
            Work::Changed(key) => match read_item(bus, &key, icon_size) {
                Some((item, menu)) => {
                    match menu {
                        Some(menu) => menus.insert(key.clone(), menu),
                        None => menus.remove(&key),
                    };
                    match items.iter_mut().find(|i| i.key == key) {
                        Some(old) if *old != item => {
                            *old = item;
                            true
                        }
                        _ => false,
                    }
                }
                None => false,
            },
            Work::Activate(key, x, y) => {
                if let Some(p) = proxy(bus, &key, None, ITEM) {
                    let _ = p.call_method("Activate", &(x, y));
                }
                false
            }
            Work::SecondaryActivate(key, x, y) => {
                if let Some(p) = proxy(bus, &key, None, ITEM) {
                    let _ = p.call_method("SecondaryActivate", &(x, y));
                }
                false
            }
            Work::Menu(key, x, y, then) => {
                match menus.get(&key) {
                    Some(path) => then(read_menu(bus, &key, path.as_str()).unwrap_or_default()),
                    None => {
                        if let Some(p) = proxy(bus, &key, None, ITEM) {
                            let _ = p.call_method("ContextMenu", &(x, y));
                        }
                    }
                }
                false
            }
            Work::Clicked(key, id) => {
                if let Some(path) = menus.get(&key)
                    && let Some(p) = proxy(bus, &key, Some(path.as_str()), MENU)
                {
                    let _ = p.call_method("Event", &(id, "clicked", Value::from(0i32), 0u32));
                }
                false
            }
        };
        if changed && !send(items.clone()) {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::zvariant::{Array, Dict, Signature, StructureBuilder};

    #[test]
    fn items_register_by_name_or_by_path_on_their_connection() {
        assert_eq!(
            item_key("org.kde.StatusNotifierItem-123-1", ":1.5"),
            "org.kde.StatusNotifierItem-123-1/StatusNotifierItem"
        );
        assert_eq!(
            item_key("/org/ayatana/NotificationItem/steam", ":1.42"),
            ":1.42/org/ayatana/NotificationItem/steam"
        );
        assert_eq!(
            split_key(":1.42/org/ayatana/NotificationItem/steam"),
            (":1.42", "/org/ayatana/NotificationItem/steam")
        );
    }

    #[test]
    fn mnemonics_leave_labels() {
        assert_eq!(label("_Library"), "Library");
        assert_eq!(label("Exit _Steam"), "Exit Steam");
        assert_eq!(label("snake__case"), "snake_case");
    }

    #[test]
    fn pixels_become_straight_rgba_at_the_size_asked() {
        // A 2x2 of opaque red, as ARGB in network order.
        let red = [255, 255, 0, 0].repeat(4);
        let Some(Icon::Pixels { size, rgba }) = pixels(&[(2, 2, red)], 2) else {
            panic!("an icon");
        };
        assert_eq!(size, 2);
        assert_eq!(&rgba[..4], &[255, 0, 0, 255]);
        // The smallest pixmap at least as big is chosen, then scaled.
        let small = (1, 1, vec![255, 0, 255, 0]);
        let big = (4, 4, [255, 0, 0, 255].repeat(16));
        let Some(Icon::Pixels { size, rgba }) = pixels(&[small, big], 2) else {
            panic!("an icon");
        };
        assert_eq!(size, 2);
        assert_eq!(&rgba[..4], &[0, 0, 255, 255], "from the blue one");
        assert_eq!(pixels(&[(2, 2, vec![0; 3])], 2), None, "too little data");
    }

    /// A dbusmenu node `(ia{sv}av)`.
    fn node(
        id: i32,
        props: &[(&str, Value<'static>)],
        children: Vec<Value<'static>>,
    ) -> Value<'static> {
        let mut dict = Dict::new(&Signature::Str, &Signature::Variant);
        for (k, v) in props {
            dict.append(
                Value::from(k.to_string()),
                Value::Value(Box::new(v.try_clone().unwrap())),
            )
            .unwrap();
        }
        let mut array = Array::new(&Signature::Variant);
        for child in children {
            array.append(Value::Value(Box::new(child))).unwrap();
        }
        StructureBuilder::new()
            .add_field(id)
            .append_field(Value::Dict(dict))
            .append_field(Value::Array(array))
            .build()
            .unwrap()
            .into()
    }

    #[test]
    fn a_menu_reads_as_entries() {
        let menu = node(
            0,
            &[],
            vec![
                node(1, &[("label", Value::from("_Library"))], vec![]),
                node(2, &[("type", Value::from("separator"))], vec![]),
                node(
                    3,
                    &[
                        ("label", Value::from("Big Picture")),
                        ("enabled", Value::from(false)),
                    ],
                    vec![],
                ),
                node(
                    4,
                    &[
                        ("label", Value::from("Show")),
                        ("toggle-type", Value::from("checkmark")),
                        ("toggle-state", Value::from(1i32)),
                    ],
                    vec![],
                ),
                node(5, &[("visible", Value::from(false))], vec![]),
            ],
        );
        let Some(MenuEntry::Entry { children, .. }) = menu_entry(&menu) else {
            panic!("a menu");
        };
        assert_eq!(
            children,
            [
                MenuEntry::Entry {
                    id: 1,
                    label: "Library".into(),
                    enabled: true,
                    checked: None,
                    children: vec![]
                },
                MenuEntry::Separator,
                MenuEntry::Entry {
                    id: 3,
                    label: "Big Picture".into(),
                    enabled: false,
                    checked: None,
                    children: vec![]
                },
                MenuEntry::Entry {
                    id: 4,
                    label: "Show".into(),
                    enabled: true,
                    checked: Some(true),
                    children: vec![]
                },
            ],
            "the hidden one left out"
        );
    }
}

/// On a bus of its own: `dbus-run-session -- cargo test -p patina-system
/// sni -- --ignored`.
#[cfg(test)]
mod bus_tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// An app's item, as libappindicator would serve it.
    struct FakeItem {
        activated: Arc<Mutex<Vec<(i32, i32)>>>,
    }

    #[zbus::interface(name = "org.kde.StatusNotifierItem")]
    impl FakeItem {
        fn activate(&self, x: i32, y: i32) {
            self.activated.lock().unwrap().push((x, y));
        }
        #[zbus(property)]
        fn id(&self) -> String {
            "steam".into()
        }
        #[zbus(property)]
        fn title(&self) -> String {
            "Steam".into()
        }
        #[zbus(property)]
        fn status(&self) -> String {
            "Active".into()
        }
        #[zbus(property)]
        fn icon_name(&self) -> String {
            "steam".into()
        }
        #[zbus(property)]
        fn menu(&self) -> OwnedObjectPath {
            OwnedObjectPath::try_from("/MenuBar").unwrap()
        }
        #[zbus(property)]
        fn item_is_menu(&self) -> bool {
            false
        }
    }

    struct FakeMenu {
        clicked: Arc<Mutex<Vec<i32>>>,
    }

    #[zbus::interface(name = "com.canonical.dbusmenu")]
    impl FakeMenu {
        fn about_to_show(&self, _id: i32) -> bool {
            false
        }
        fn get_layout(&self, _parent: i32, _depth: i32, _props: Vec<String>) -> (u32, Layout) {
            let entry = |id: i32, label: &str| -> OwnedValue {
                let props: HashMap<String, OwnedValue> = [(
                    "label".to_string(),
                    OwnedValue::try_from(Value::from(label)).unwrap(),
                )]
                .into_iter()
                .collect();
                let children: Vec<OwnedValue> = Vec::new();
                OwnedValue::try_from(Value::from((id, props, children))).unwrap()
            };
            (
                1,
                (
                    0,
                    HashMap::new(),
                    vec![entry(1, "_Library"), entry(2, "E_xit Steam")],
                ),
            )
        }
        fn event(&self, id: i32, kind: &str, _data: OwnedValue, _time: u32) {
            if kind == "clicked" {
                self.clicked.lock().unwrap().push(id);
            }
        }
    }

    fn wait<T>(what: &str, mut check: impl FnMut() -> Option<T>) -> T {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(found) = check() {
                return found;
            }
            assert!(Instant::now() < deadline, "no {what} within 5 s");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    #[ignore = "needs a session bus of its own"]
    fn an_apps_item_shows_activates_and_its_menu_clicks() {
        let seen: Arc<Mutex<Vec<Item>>> = Arc::default();
        let into = seen.clone();
        let tray = start(22, move |items| {
            *into.lock().unwrap() = items;
            true
        })
        .unwrap();

        let activated: Arc<Mutex<Vec<(i32, i32)>>> = Arc::default();
        let clicked: Arc<Mutex<Vec<i32>>> = Arc::default();
        let app = zbus::blocking::connection::Builder::session()
            .unwrap()
            .serve_at(
                DEFAULT_ITEM_PATH,
                FakeItem {
                    activated: activated.clone(),
                },
            )
            .unwrap()
            .serve_at(
                "/MenuBar",
                FakeMenu {
                    clicked: clicked.clone(),
                },
            )
            .unwrap()
            .build()
            .unwrap();
        // libappindicator's way: its own connection and an object path.
        let watcher = Proxy::new(&app, WATCHER, WATCHER_PATH, WATCHER).unwrap();
        watcher
            .call_method("RegisterStatusNotifierItem", &(DEFAULT_ITEM_PATH,))
            .unwrap();

        let item = wait("item", || seen.lock().unwrap().first().cloned());
        assert_eq!((item.id.as_str(), item.title.as_str()), ("steam", "Steam"));
        assert_eq!(
            item.icon,
            Some(Icon::Named {
                name: "steam".into(),
                theme_path: None
            })
        );
        assert!(item.has_menu);
        assert_eq!(
            watcher
                .get_property::<Vec<String>>("RegisteredStatusNotifierItems")
                .unwrap(),
            std::slice::from_ref(&item.key)
        );

        tray.activate(&item.key, 100, 200);
        wait("activation", || activated.lock().unwrap().first().copied());
        assert_eq!(activated.lock().unwrap()[0], (100, 200));

        let (menu_tx, menu_rx) = mpsc::channel();
        tray.menu(&item.key, 0, 0, move |entries| {
            let _ = menu_tx.send(entries);
        });
        let entries = menu_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let labels: Vec<_> = entries
            .iter()
            .filter_map(|e| match e {
                MenuEntry::Entry { label, .. } => Some(label.as_str()),
                MenuEntry::Separator => None,
            })
            .collect();
        assert_eq!(labels, ["Library", "Exit Steam"]);

        tray.clicked(&item.key, 2);
        wait("click", || clicked.lock().unwrap().first().copied());
        assert_eq!(clicked.lock().unwrap()[0], 2);

        // The app leaving the bus takes its item with it.
        drop(watcher);
        drop(app);
        wait("removal", || seen.lock().unwrap().is_empty().then_some(()));
    }
}
