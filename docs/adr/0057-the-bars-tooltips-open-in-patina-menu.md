# 0057. The bar's tooltips open in patina-menu too

Date: 2026-10-17

## Status

Accepted. The user chose to have tooltips as little overlays from a separate program, the way
the bar's menus work (ADR-0056).

## Context

egui draws tooltips inside the bar's 41 px surface, so they were cut off, as its menus had been.
The bar's tooltips:

- the window tabs' titles, which matter most with icon-only tabs (ADR-0050);
- the tray's Wi-Fi strength, volume, battery and power button.

ADR-0056 left them out: a tooltip appears and goes as the pointer moves, which fits a
long-running process less well than a menu does. The alternative was growing the bar's surface
while it's hovered, with click-through room for egui's own tooltips. The user preferred an
overlay program.

## Decision

- **`patina-menu --tooltip TEXT --at X,Y --edge top|bottom`** shows `TEXT` centred on X, past Y.
  It's on a click-through surface that takes no keyboard, and it wears the bar's skin. It stays
  until its standard input closes, so whoever starts it ends it by closing the pipe, or by
  exiting. There's no kill, and no tooltip left behind by a crashed bar.
- **The bar notes what's hovered** (`bar/src/tooltip.rs`): the tray calls `tooltip::hover`, and
  Patina's `TabStrip` gains `own_tooltip`, which reports the hovered tab and its rectangle.
  After each frame, a small state machine (`Tips`) decides what to do. After 500 ms of hovering
  the same thing, it shows the tooltip, centred under or over it, past the bar's edge. Hovering
  something else, or nothing, hides it.
- **egui's own tooltips are off in the bar** (an infinite `tooltip_delay`), so nothing is drawn
  cut off. That includes the ones in Patina's widgets.

## Consequences

- The bar's tooltips are whole, wherever the bar is, and look like the menus.
- Each tooltip starts a process, so it appears a little after the delay. The bar starts it as its
  own child, as it does a tab's menu, so it isn't in Settings' Processes list.
- A new tooltip in the bar has to go through `tooltip::hover`; `on_hover_text` there shows
  nothing.
- Like the menus, a tooltip opens on the output the compositor chooses.
