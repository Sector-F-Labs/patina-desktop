# 0029. A right click on the desktop opens the desktop's menu, which chooses the shell's skin

Date: 2026-10-15

## Status

Accepted; the skin list was replaced by the style in [0105](0105-the-desktop-menu-chooses-the-style-not-the-skin.md)

## Context

The shell's skin is the bar's `skin` key: the bar reads it, passes it to the pieces it opens as
`PATINA_SKIN`, and pieces the compositor starts read it from the bar's config. Changing it meant
editing the file and restarting the bar. Patina apps change skin from a Skin menu (the skins, Copy
built-in skins to folder, Open skins folder), which the user wanted for the shell too.

A settings app was considered, and will likely come, but would still only edit config files; for now
the user asked for the skin menu on a right click on the desktop, as desktops put their settings
there. Putting it in the bar's own menus was ruled out: the bar has no menu to grow, and the skin is
the whole shell's, not the bar's.

## Decision

- A right click on the desktop (no window or layer surface under the pointer, Super not held) runs
  the compositor's `desktop_menu` (default `patina-menu`) with `--at X,Y`, the pointer in logical
  pixels.
- **patina-menu** is a new piece: a transparent overlay surface over the whole screen, with Patina's
  menu frame at the pointer, kept on screen. It closes on a click elsewhere, Escape, or losing the
  keyboard. It lists the skins (Patina's built-ins and the bar's skins folder) with the one in use
  chosen, then Copy built-in skins to folder, Open skins folder and Open bar settings.
- Choosing a skin writes `skin = "…"` into `~/.config/patina-hud/config.toml`, keeping the rest of
  the file (or making it), and restyles the menu at once. The bar checks the file's modification
  time each second, on the clock's tick, and restyles itself when the skin in it changes
  (`Shell::restyle`). Pieces read the skin when they open, now also from the bar's skins folder.

## Consequences

- The shell's skin can be changed and tried without editing a file or restarting anything.
- The bar's config stays the one place the skin lives; a settings app can edit it the same way.
- The bar reloads only the skin from a changed file; other keys still need the bar restarted.
- Light and Dark for skins that follow the desktop, which Patina apps offer, aren't here: the shell
  has no setting for them yet, and follows the desktop's colour scheme.
- The menu's surface covers the screen, so the first click outside it only closes it.
