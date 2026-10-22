# 0080. Apps pin to the bar, and its tabs drag into another order the compositor keeps

Date: 2026-10-23

## Status

Accepted. The user asked for apps to be pinned in the bar and for its tabs to be dragged into
another order. Asked what pinning should mean, they chose a dock-style launcher over a browser's
pinned tab. The rest is my design.

## Context

The bar's tabs are the one window list (ADR-0027). They show the windows in the order the
compositor announced them over wlr-foreign-toplevel (ADR-0012), which is the order they opened.
The compositor's Super+1 to 9 and Alt+Tab use the same list, so the tab a key reaches is the one
it names. Any new order has to reach both, or Super+3 stops matching the third tab.

Two meanings of "pin" were on the table:

- **A dock-style launcher.** The app keeps a place on the bar while it isn't running, and a click
  starts it.
- **A browser's pinned tab.** A window's tab moves to the front and shrinks to its icon, but goes
  when the window does.

The user chose the launcher.

Where the order lives was my choice. These options were considered:

- **The bar keeps the order alone.** That's simplest, but the compositor's keys would no longer
  follow the tabs, against ADR-0027.
- **The compositor re-announces windows in the new order** (closing and re-creating their handles).
  Every taskbar, waybar's included, would see windows close and open again.
- **The compositor owns the order, and the bar asks it to move a window.** The protocol has no
  order and no ids shared with D-Bus. But both sides have the same list in the same order, so an
  index names the same window to both.

## Decision

- **Pins are `pinned` in the bar's config**, a list of app ids. The tab menu's Pin to Bar and
  Unpin from Bar write it (`patina_system::pins::set`, with toml_edit, keeping the rest of the
  file).
- **The tab order is worked out from the pins and the windows, the same way on both sides**
  (`patina_system::pins::arrange`). Each pinned app comes in its place, as its windows' tabs, or
  as a launcher when none is open. The other windows follow. The compositor reads `pinned` from
  the bar's config, at start-up and within a second of a change, as it already does the skin for
  its X11 frames. A launcher is a faded icon tab (Patina's `TabStrip::launchers`, its ADR-0021).
  A click on it, Open in its menu, or its Super+number starts the app by its `.desktop` entry's
  Exec (`apps::start_command`).
- **Dragging a tab** (Patina's `TabStrip::movable`) does one of three things
  (`bar/src/pins.rs`):
  - A pinned app's tab moves the app among the pins.
  - Another window's tab dropped among the pins pins its app there.
  - Dropped among the other windows, it moves along them.
- **The compositor keeps the windows' order.** The bar calls `Move(from, to)` on
  `com.sectorflabs.Patina.Tabs` with indices into the list of windows as announced. The compositor
  reorders its list and signals `Moved(from, to)`, and the bar applies the same move to its own
  copy. The bar keeps its windows in that order as they change, with new ones after. Where nothing
  serves the name (another compositor), the bar moves the tab itself.
- The split bar's column of icons (experimental) shows the pins and launchers too, but doesn't
  drag.

## Consequences

- Super+1 to 9, Alt+Tab and the neighbour focused after a close all follow the tabs as they're
  dragged and pinned.
- Pins persist in the bar's config. The window order lasts only as long as the windows do.
- An index is only right while both lists agree. A window opening or closing between a drag and
  its `Moved` could move the wrong one. That race lasts milliseconds, and the result is only a
  tab in the wrong place.
- Pins are matched on the exact app id, so an app whose X11 class differs from its Wayland id
  pins as two apps.
- The compositor reads one more key from the bar's config file. A bar started with `-c` elsewhere
  pins into its own file, which the compositor doesn't read.
- A pinned app's windows keep their own order inside its group. Dragging one of them moves the
  whole app.
- Revisit if `ext-foreign-toplevel-list`'s identifiers reach window management. Ids would then
  replace indices.
