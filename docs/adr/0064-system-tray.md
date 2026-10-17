# 0064. The bar is the system tray, by StatusNotifierItem

Date: 2026-10-18

## Status

Accepted. The user asked for a working system tray, for apps such as Steam.

## Context

Apps like Steam, chat clients and sync tools put an icon in the system tray, with a menu. The bar's
tray had only the desktop's own icons (Wi-Fi, volume, battery). Steam was running, but there was
nowhere for its icon to go.

Two protocols exist:

- **XEmbed**, X11's: the icon is a window embedded in the tray's window. It needs an X11 tray
  (`_NET_SYSTEM_TRAY_S0`) that owns and reparents the windows, which a Wayland bar isn't.
- **StatusNotifierItem**, the freedesktop and KDE protocol on D-Bus, which libappindicator and
  Ayatana speak. Apps register with `org.kde.StatusNotifierWatcher`, describe their icon in
  properties (by name and theme folder, or as pixels), and serve their menu by
  `com.canonical.dbusmenu`. GNOME (with its AppIndicator extension), KDE, waybar and Sway's bars
  host it. Steam uses it whenever a watcher is on the bus. With none, its icon had nowhere to
  go.

## Decision

- **`patina_system::sni`.**
  - It serves `org.kde.StatusNotifierWatcher` at `/StatusNotifierWatcher`, and keeps the
    registered items. Registration is by bus name, or by object path on the sender's
    connection, as libappindicator does it.
  - It drops an item when its program leaves the bus, and signals as the protocol says.
  - If another program already owns the name, it follows that watcher instead.
  - It reads each item's properties afresh: icon by name with its theme folder, or pixels in
    ARGB scaled to the bar's icon size; status; title; tooltip; whether it's only a menu; and
    its dbusmenu path. It reads them again on the item's `New…` signals.
  - It gives the bar each change as the whole list, on threads of its own. `Tray` activates an
    item and reads and clicks its menu. An item without a dbusmenu is asked to show its own
    (`ContextMenu`).
- **The bar's `tray` module shows the items first**, in whichever process has the tray. In split
  mode (ADR-0063), that's the tray part. Passive items are hidden.
  - Each is a button like Wi-Fi's, with its icon at the exact pixel size on whole pixels.
    Named icons come from the item's own folder first, then the icon theme, through the bar's
    icon thread.
  - A click activates the app, or opens the menu of an item that's only a menu. A middle click
    is its secondary action, and a right click opens its menu in patina-menu at the icon
    (ADR-0056). Hovering shows its tooltip or title.
- **A menu in patina-menu** is its entries in order:
  - submenus folded in as "Parent › Item";
  - checked entries with a tick;
  - disabled ones left out, as patina-menu can't grey them yet.

  The chosen entry is sent back as dbusmenu's `Event(id, "clicked")`.
- `cargo run -p patina-system --example tray` prints what's registered, and the items' menus. A
  bus test has a fake app register, get activated, and have its menu read and clicked.

## Consequences

- Steam, and other apps that use libappindicator, show their icons and menus on the bar.
- XEmbed-only apps (old GTK2 or Wine trays) still show nothing.
- Menus lose what patina-menu can't show: submenus are folded in, disabled entries are hidden,
  and icons on entries are dropped.
- Monochrome icons such as Steam's are drawn in their own colours, so they can be faint on a light
  skin.
- Whichever bar process holds the tray owns the watcher name. When the bar restarts, apps
  register again with the new one; libappindicator watches for that.
