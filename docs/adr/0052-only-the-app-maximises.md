# 0052. Only the app's maximise button maximises; tiling up fills

Date: 2026-10-17

## Status

Proposed. The user asked that only the maximise button maximise an app. Tiling a window upward
should fill the screen, but not as an explicit maximise, so that it looks right with gaps
(ADR-0051). With gaps on, the app's maximise fills too: amended by
[ADR-0093](0093-with-gaps-maximising-fills.md).

## Context

Dragging a window to the top edge, Super+Up, and the app's own maximise button all did the
same thing: a `Maximized` placement and the xdg `maximized` state. Apps draw a maximised window
differently. They square its corners and drop its shadow and resize borders, because it's
meant to meet the screen's edges. With gaps on, a window tiled upward sits inside a gap, so
those square corners floating in the gap looked wrong.

The tiled states had the same problem. They were measured against the area inside the gaps, so a
tiled half was told it was tiled on three sides and squared those corners, with nothing next to
it.

## Decision

- **Two placements.**
  - `Maximized` is only for the app's own request: its maximise button, a double click on its
    title bar, or the bar's window menu, which asks the same through
    wlr-foreign-toplevel. It fills the work area edge to edge, gaps or not, and the app is told
    it's maximised.
  - `Fill` is tiling upward: dragging to the top edge, or Super+Up. It fills the tiling area
    (inside the gaps) as a tiled window.
- **The tiled states are measured against the work area**, so a side is tiled only when it
  really meets the screen's or the bar's edge. Without gaps, a filled window is tiled on all
  four sides and a half on three, as before. With gaps, nothing touches, so tiled windows keep
  their rounded corners and shadows.
- **On D-Bus**, a filled window's placement is `"fill"`. `Place` with the whole tiling area fills
  the window rather than maximising it.

## Consequences

- With gaps, a filled window looks like a window, and a maximised one looks maximised.
- Super+Up no longer maximises, so an app's maximise button, which says it's maximised, can
  differ from what Super+Up shows.
- Without gaps, filling and maximising cover the same area. Apps still draw them differently:
  a filled window is tiled on all four sides rather than maximised.
