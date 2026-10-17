# patina-menu

The desktop's menu, opened by a right click on the empty desktop (ADR-0029). It's the quick way
to style the whole shell: the skin every piece wears, and the way into Settings (ADR-0038), where
the wallpaper is chosen (ADR-0030). It's also a menu of any items (ADR-0056), for pieces too
small to hold their own menus: the bar's window tabs open theirs in it. And it's the bar's
tooltips (ADR-0057).

## On the desktop

At the pointer: the skins (Patina's built-ins and your own); Wallpaper…, which opens Settings at
its Wallpaper page; items to copy the built-in skins to the bar's skins folder, open that folder,
and open the bar's settings file; and Settings…. A click elsewhere, Escape or losing the keyboard
closes it.

## How it fits in

- **Started by** patina-compositor on a right click where no window is, as
  `patina-menu --at X,Y` (its `desktop_menu` command), at the pointer in logical pixels.
- **Starts patina-settings**, found on the compositor's PATH like the menu itself: through the
  compositor's `Spawn` on the session bus, so it's among the desktop's processes (ADR-0045), or
  itself where nothing serves that.
- **Writes the skin** into the bar's config, `~/.config/patina-bar/config.toml`, through
  patina-shell (`bar::set`). It doesn't tell anyone: the bar and
  patina-wallpaper watch the file and change at once, and the launcher, menus and lock screen
  read it when they next open.
- **Drawn** on a transparent layer-shell surface over the whole screen, so the menu can sit at
  the pointer and stay on screen near the edges.
- **A menu of items** with `--item ID=LABEL` (and `--separator`): it prints the chosen ID and
  exits, or prints nothing if closed. `--edge bottom` counts Y from the bottom and opens upward,
  as patina-bar asks for a tab's menu (`bar/src/menu.rs`); `--edge left` or `right` counts X
  from that side and opens outward, for a split bar's columns (ADR-0063).
- **A tooltip** with `--tooltip TEXT`: click-through, no keyboard, centred on X; it goes when its
  standard input closes, which is how patina-bar hides it (`bar/src/tooltip.rs`).

## Code

`main.rs` (the arguments), `overlay.rs` (placing and closing, for every menu), `ui.rs` (the
desktop's menu), `items.rs` (a menu of items), `tooltip.rs`.

## Running

```sh
cargo run -p patina-menu -- --at 200,150
cargo run -p patina-menu -- --at 300,45 --edge bottom --item new=New --item close=Close
```
