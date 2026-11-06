# 0113. The HUD's parts ask the compositor for shadows, and `shadows` in the HUD config turns them off

Date: 2026-11-06

## Status

Accepted. The user asked for shadows on the HUD's elements, optional, with the choice on Settings'
HUD page. It follows [0112](0112-windows-ask-for-their-shadow-through-the-kde-shadow-protocol.md).

## Context

Patina's windows have a shadow (ADR-0112); the HUD's parts, the launcher, the window list, the tray
and the media controls, are rounded islands on layer-shell surfaces and had none. Each part is its
own `patina-hud` process with surfaces made by patina-shell's `Host` (ADR-0063, ADR-0110).

Ways to give them one:

- **Draw it inside the surface**, in a margin around the island. The surface would then be bigger
  than the island by the shadow's reach: its exclusive zone, anchors, margins and the hot strip it
  hides into (ADR-0111) would all need to account for it, and the surface would cover pixels that
  belong to the windows. Rejected.
- **The compositor shadows every layer surface.** It can't tell a wallpaper, a menu or a lock
  screen from an island, and would shadow all of them. Rejected, as it was for windows.
- **Ask for it, through the protocol Patina apps use** (ADR-0112). patina-shell has its own Wayland
  connection, so it binds the shadow manager like any other global, which an app under winit can't
  do. The compositor, which already draws shadows for windows that ask, draws them for layer surfaces
  that ask. Chosen.

One thing is particular to the HUD. A part's surface is bigger than the island it draws: a transparent
strip along its inner side, `hide::HOT` (10 px), is where the pointer brings a hidden part back
(ADR-0111). A shadow placed around the surface sat 10 px out from the island, and so looked heavier
and farther away than a window's: 113 of 255 dark beside the tray against a window's 61, on a white
wallpaper.

## Decision

- **patina-shell's `Host::set_shadow(Option<ShadowStyle>)`** asks for a shadow on every layer surface,
  and on the ones that appear later (a new output's), or takes them away. It makes the eight tiles
  with patina-ui's `frame::shadow::tiles` (Patina's ADR-0030) and hands them over in a pool of its own
  that lives as long as the shadow. The compositor draws a layer surface's shadow under it, with
  `shadow::layer_elements`, beside `frames::desktop_elements`'s windows'.
- **`Shell::shadow_inset()`** says how far the drawn shape is inside the surface (top, right, bottom,
  left). The shell takes that off the shadow's offsets, so it goes around the shape. The HUD answers
  with its hot strip, as `hot_trim` already gives it for the island it draws.
- **`shadows`** in the HUD's config, `true` by default, turns them off. The HUD applies it at once
  from its config watcher, and Settings' HUD page has the checkbox, which writes it with
  `patina_shell::hud::set_bool`. A reserve (the invisible surface that only keeps an edge's space)
  never has one.
- **How they look is the config's too**, since the right size and strength will take trying:
  `shadow_blur`, `shadow_opacity`, `shadow_drop`, `shadow_offset_x`, `shadow_spread` and
  `shadow_color` (`#rrggbb`), which fill patina-ui's `shadow::Style` (Patina's ADR-0030). The defaults are
  the windows': blur 8, opacity 0.5, drop 3, the rest 0 and black. Settings' HUD page has a slider for each, a
  colour picker and a reset button; a slider is written when it's let go, and the HUD follows within a
  second. Sizes are clamped (64 px at most) so a config can't ask for huge tiles.
- **A part that has slid right off the screen has no shadow** (`Shell::shadow_hidden`, answered by the
  HUD when `Slide::out()` is 0): the shadow would reach back onto the screen from beside its edge, with no
  part to be the shadow of.
- **The compositor's `[shadows] enabled`** (ADR-0112) is a switch for all of them: with it off no
  shadow is drawn, whatever the HUD asks.

## Consequences

- The HUD's islands have the same shadow as windows: on a white wallpaper 61, 54, 48 of 255 dark
  moving out from the edge of the tray, as a window's sides, and 42, 37, 31 above the window list, as a
  window's top.
- Checked in the Linux VM, on a white wallpaper: with the tray and the window list set to autohide and
  away, nothing is left along the screen's edges, and the launcher button, which stays, keeps its
  shadow. While a part slides its shadow goes with it, and it appears or goes at the ends of the slide;
  I haven't watched that.
- Changing the keys in the config changes the shadows within a second (checked with a tight, strong,
  red one, and with `shadows = false`).
- The defaults are placeholders. The user will try values and come back with the ones to keep.
- Layer surfaces are not dimmed when unfocused, as windows are: they take no focus.
- An island narrower than about 32 px loses the middle of its top and bottom shadow, as the tiles'
  corners meet: the media controls' 1 px placeholder for nothing playing has none.
- Other pieces built on patina-shell, such as the launcher and menus, could ask the same way; none
  does yet.
- `Shell::shadow_inset` is a second place that knows about the hot strip, beside `wanted_input_trim`
  and the drawing.
