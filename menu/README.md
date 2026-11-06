# patina-menu

The desktop's menu, opened by a right click on the empty desktop (ADR-0029). It's the quick way
to set the style, light or dark, and the way into Settings (ADR-0038), where the skin (ADR-0105)
and the wallpaper (ADR-0030) are chosen. It's also a menu of any items (ADR-0056), for pieces too
small to hold their own menus: the bar opens its tabs' menus, its tray items' menus (ADR-0064)
and the keyboard layouts' (ADR-0068) in it. And it's the bar's tooltips (ADR-0057).

How the jobs share one program, with diagrams, is in [HOW-IT-WORKS.md](HOW-IT-WORKS.md).

## On the desktop

At the pointer: the style, Default, Light or Dark; Wallpaper…, which opens Settings at
its Wallpaper page; items to copy the built-in skins to the bar's skins folder, open that folder,
and open the bar's settings file; and Settings…. A click elsewhere, Escape or losing the keyboard
closes it.

## How it fits in

- **Started by** patina-compositor on a right click where no window is (and Super isn't held), as
  `patina-menu --at X,Y` (its `desktop_menu` command), at the pointer in logical pixels.
- **Starts patina-settings**, found on the compositor's PATH like the menu itself: through the
  compositor's `Spawn` on the session bus, so it's among the desktop's processes (ADR-0045), or
  itself where nothing serves that.
- **Sets the style** through `gsettings` (GNOME's `color-scheme`), as Settings does; the portal
  hands it on and every piece that follows it changes at once. It no longer chooses the skin:
  that's Settings' Appearance page.
- **Drawn** on a transparent layer-shell surface over the whole screen, so the menu can sit at
  the pointer and stay on screen near the edges.
- **A menu of items** with `--item ID=LABEL` (and `--separator`): it prints the chosen ID and
  exits, or prints nothing if closed. patina-hud asks for one for a window tab (New Window, Pin
  or Unpin, Close Window), a tray item (its entries, submenus shown as "Friends › Online") and
  the keyboard layouts (the one in use ticked), each as its own child, to read the answer
  (`bar/src/menu.rs`). Submenus, icons and greyed entries aren't possible. `--edge bottom` counts Y from the bottom and opens upward,
  as patina-hud asks for a tab's menu (`bar/src/menu.rs`); `--edge left` or `right` counts X
  from that side and opens outward, for a split bar's columns (ADR-0063).
- **A tooltip** with `--tooltip TEXT`: click-through, no keyboard, centred on X; it goes when its
  standard input closes, which is how patina-hud hides it, after hovering for half a second
  (`bar/src/tooltip.rs`).

## Code

`main.rs` (the arguments), `overlay.rs` (placing and closing, for every menu), `ui.rs` (the
desktop's menu), `items.rs` (a menu of items), `tooltip.rs`.

## Running

```sh
cargo run -p patina-menu -- --at 200,150
cargo run -p patina-menu -- --at 300,45 --edge bottom --item new=New --item close=Close
```
