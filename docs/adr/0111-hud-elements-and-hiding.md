# 0111. HUD elements hide at their edge

Date: 2026-11-06

## Status

Accepted

Follows [0110](0110-the-bar-becomes-the-hud.md) and [0063](0063-split-bar.md).

## Context

Once the HUD is its elements on surfaces of their own (ADR-0063, ADR-0110), each should be
configurable apart: left out, or kept off screen until the pointer goes where it was put, so a
window can use the whole screen. One key should do that for all of them at once.

## Decision

- Split is the default layout. Each element (`launcher`, `windows`, `tray`, `media`) has a
  `*_mode`: `always`, `autohide` or `hidden`. `launcher_at = "none"` still means `hidden`.
- An autohiding element slides along its edge, by its layer-shell margin, until two pixels of
  it are left; the pointer on that sliver slides it back, and it stays 0.6 s after the pointer
  leaves. A hidden element isn't started, except the window list, which the others hang from
  and which goes off screen entirely.
- Only `always` elements keep space (the reserve sections, ADR-0063). An element that hides
  keeps none, so windows use it, and it opens over them.
- **Super+H** (`hud` in `[keys]`) runs `patina-hud --toggle`, which makes or removes
  `$XDG_RUNTIME_DIR/patina-hud/hidden`. Every process looks for the file 6 times a second:
  with it, `always` elements act as `autohide`, and the reserve sections give their zone back.
  `autohide` and `hidden` elements are as they were. The state is a file because the
  processes need no connection to each other for it, and it ends with the session.
- The host asks the shell for a margin and an exclusive zone after each frame, as it does for
  the size (`Shell::wanted_margin`, `wanted_exclusive_zone`).

## Consequences

- Super+H was the app's, as Ctrl+H (ADR-0067); now it hides the HUD. `hud = ""` in `[keys]`
  frees it.
- The single bar (`layout = "bar"`) doesn't hide.
- Menus and tooltips of an element that has slid away open at its place in the layout, as the
  HUD can't know where a half-hidden element is on screen mid-slide; they only open on a
  click or a hover, when it is out.
- Settings' page still places only the elements, not their modes.
