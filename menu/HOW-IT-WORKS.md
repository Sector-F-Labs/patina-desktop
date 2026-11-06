# How the desktop's menu fits in

Right-click the empty desktop and a small menu appears at the pointer. Right-click a tab on
the bar, or an icon in its tray, and a menu appears just past the bar; hover one for half a
second, and a tooltip does. This page explains why all of those are the same small program, and
how it is placed and closed. The README next to it covers using and running it; the decisions are
in ADR-0029, ADR-0056 and ADR-0057.

## The idea in one paragraph

A layer-shell surface can only draw inside itself, and the bar's is 41 pixels tall. egui draws
its menus and tooltips as areas in the same surface, so any menu the bar opened was cut off at
its edge. Growing the bar's surface, or splitting an egui frame across two surfaces, was the
fiddly way out. The easy one was already in the tree: the desktop's menu is a transparent
surface over the whole screen, so it can sit anywhere and stay on screen. `patina-menu` is that,
made to take its contents from the command line. The bar names the items; `patina-menu` draws
them and prints which one was chosen.

## The picture

<img src="images/architecture.svg" alt="The compositor opens patina-menu's desktop menu on a right click on the empty desktop; it sets the style (default, light or dark) through gsettings and opens Settings. The bar opens menus of items for window tabs, tray items and keyboard layouts and reads the chosen ID from standard output; it also shows tooltips, which end when their standard input closes.">

One program, three jobs, chosen by its arguments:

- **No items: the desktop's menu.** The style (default, light or dark), Wallpaper…, copy
  the built-in skins to the skins folder, open it, open the bar's settings file, and Settings….
- **`--item ID=LABEL`, `--separator`: a menu of items.** It prints the ID of the one chosen, or
  nothing, and exits.
- **`--tooltip TEXT`: a tooltip.** No clicks, no keys, until its standard input closes.

## Where it opens

<img src="images/overlay.svg" alt="A screen with a bar along the bottom and a transparent surface over all of it. The desktop menu opens with its top left corner at the pointer. A menu of items opens just above the bar with --at 300,45 --edge bottom, counting 45 pixels up from the bottom. A tooltip sits centred above the tray. A list says which way each --edge value opens.">

The surface is on the overlay layer, anchored to all four edges, with an exclusive zone of -1 so
it covers bars too. An interactive menu takes the keyboard; a tooltip takes neither the keyboard
nor the clicks. `overlay.rs` places the menu for every job, kept on screen near an edge:

- **`--at X,Y`**, in the screen's logical pixels, and **`--edge`** to say what they count from.
  `top` is the default, `bottom` counts Y up from the bottom and opens upward, which is how a
  bar at the bottom asks for a menu without knowing the screen's height; `left` and `right`
  count X from that side and open outward, for a split bar's columns (ADR-0063).
- **Closing** is the same for each menu: Escape, a click on the transparent part (the layer
  under the pointer is the background), or losing the keyboard after having it.

## Who asks, and what comes back

| Who | What it opens | What comes back |
|-----|---------------|-----------------|
| The compositor, on a right click where no window is, without Super held | the desktop's menu, `desktop_menu --at X,Y` (the config can change the command) | nothing on stdout; it sets the style itself, and starts Settings |
| The bar, a window tab's right click | New Window, Pin or Unpin, Close Window; Open and Unpin for a pinned app with no window (`hud/src/menu.rs`) | the item's ID, mapped back to a choice |
| The bar, a tray item's right click | its menu, read over D-Bus (ADR-0064): submenus shown as "Friends › Online", ticks as ✔, entries that can't be chosen left out | the entry's number, handed back to the item |
| The bar, the keyboard layout | each layout, the one in use ticked (ADR-0068) | the layout's index, which the bar switches to |
| The bar, hovering 500 ms (`hud/src/tooltip.rs`) | a tooltip, past the bar's edge | nothing; the bar closes its stdin when the hover ends |

The bar runs it as its own child, not through the compositor's `Spawn`, because it has to read the
answer. Settings, which the desktop's menu starts, does go through `Spawn`, so it is among the
desktop's processes (ADR-0045).

## The desktop's menu and the style

Choosing default, light or dark sets GNOME's `org.gnome.desktop.interface color-scheme` through
`gsettings` (ADR-0087, ADR-0105), the same setting Settings' Appearance page changes.
xdg-desktop-portal hands it on, and `patina::appearance::watch` follows it, so Patina apps on the
Default skin, GTK 4 and Firefox change at once, the menu among them. The skin is chosen in
Settings, not here.

Every menu here wears the bar's skin, not the app's, so a tab's menu looks like the bar it came
from.

## What it can't do

- **Opening is a process start**, so a menu or a tooltip appears a moment after the click.
- **Items are strings on a command line.** There are no submenus, icons or greyed entries, which
  is why the tray's flatten submenus into "Friends › Online" and leave out the ones that can't
  be chosen.
- **Tooltips** are only used by the bar so far. New ones there have to go through `tooltip::hover`; egui's
  own `on_hover_text` in the bar shows nothing, since its delay is infinite.
- **It opens on the output the compositor chooses**, not necessarily the one under the bar.

## The files

```
 patina-desktop/
 ├── menu/src/
 │   ├── main.rs      the arguments, and the surface over the whole screen
 │   ├── overlay.rs   placing a menu, and closing it: shared by every job
 │   ├── ui.rs        the desktop's menu: style, Wallpaper…, folders, Settings…
 │   ├── items.rs     a menu of items; prints the one chosen
 │   └── tooltip.rs   a tooltip; ends when its stdin closes
 ├── compositor/src/state.rs   a right click on the empty desktop: open_desktop_menu
 └── hud/src/
     ├── menu.rs      the arguments for a tab's, a tray item's and the layouts' menus
     └── tooltip.rs   when to show a tooltip, and the pipe that ends it
```

The diagrams in `images/` are hand-drawn SVG, and follow the viewer's light or dark theme.
