# patina-menu

The desktop's menu, opened by a right click on the empty desktop (ADR-0029). It's where the
whole shell is styled: the skin every piece wears, and the wallpaper (ADR-0030).

## On the desktop

At the pointer: the skins (Patina's built-ins and your own); a Wallpaper submenu with the
default "Sector F Labs", the images in `~/Pictures/Wallpapers` and the system's backgrounds, and
"Open wallpapers folder"; and items to copy the built-in skins to the bar's skins folder, open
that folder, and open the bar's settings. A click elsewhere, Escape
or losing the keyboard closes it.

## How it fits in

- **Started by** patina-compositor on a right click where no window is, as
  `patina-menu --at X,Y` (its `desktop_menu` command), at the pointer in logical pixels.
- **Writes the shared settings** into the bar's config, `~/.config/patina-bar/config.toml`,
  through patina-shell (`set_skin`, `set_wallpaper`). It doesn't tell anyone: the bar and
  patina-wallpaper watch the file and change at once, and the launcher, menus and lock screen
  read it when they next open.
- **Drawn** on a transparent layer-shell surface over the whole screen, so the menu can sit at
  the pointer and stay on screen near the edges.

## Code

`main.rs`, `ui.rs` (the menu and submenus), `wallpapers.rs` (finding the images on offer).

## Running

```sh
cargo run -p patina-menu -- --at 200,150
```
