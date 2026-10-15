# 0032. Patina's own cursors, by name through cursor-shape, and the compositor's during a grab

Date: 2026-10-15

## Status

Accepted

## Context

On the hardware the compositor drew one arrow, the installed theme's (`XCURSOR_THEME`) or a crude
drawn one, for every shape a client named: no I-beam over text, no hand over links, no resize
arrows. It didn't offer cursor-shape (`wp_cursor_shape_v1`), which modern clients (GTK 4, Qt 6,
Firefox, Chromium, winit and so egui, Patina's own apps and pieces) use to ask the compositor for a
shape by name instead of drawing their own. The user asked for a cursor that feels modern.

The user also saw Patina apps' resize arrows turn back into the arrow as soon as a resize from an
edge began, where Firefox kept them: during a move or resize the compositor holds the pointer, and
a client can still change its cursor (winit does), so what showed depended on the client.

Shipping an xcursor theme was ruled out: it's a build of PNGs per size, and Patina's scale is
fractional (1.2 on the laptop), which a theme's fixed sizes don't match.

## Decision

- **Patina's cursors** (`compositor/src/cursor.rs`): twelve shapes (arrow, hand, I-beam, four
  resize arrows, move, crosshair, not-allowed, busy, and arrow-with-busy) drawn as SVG on a 32-unit
  grid, rendered with resvg at the exact pixel size for the output's scale (the size times the
  scale), so they're sharp at any scale. A near-black body with a crisp white edge, rounded joins,
  and a soft drop shadow. Every name a client can ask for maps to one; they're made as first asked
  for and kept.
- The compositor offers **cursor-shape**, so clients asking by name get these; clients that send
  their own cursor image (older toolkits) still show theirs.
- **During a move or resize the compositor shows its own cursor**: the two-headed arrow along the
  edges being dragged, or the arrow while a window moves, whatever the client asks for, until the
  button is let go.
- `[cursor] theme` names an installed theme to use instead, by the same names (falling back to
  Patina's for shapes it lacks), and `[cursor] size` (24) the size at scale 1. Both apply on
  Super+Shift+R. The `XCURSOR_THEME`/`XCURSOR_SIZE` environment no longer chooses it.

## Consequences

- One consistent, modern pointer across Patina's pieces and modern apps, with the right shape for
  what's under it, sharp at fractional scales.
- Resizing from an edge keeps its arrow in every app.
- Apps that draw their own cursor image (some GTK 3 and X11 apps under Xwayland, if added) still
  use the system theme's look.
- The busy cursors are still, not spinning.
- Nested (in a window), the parent desktop draws the pointer, as before.
