# 0131. The HUD's bars fuse flush with the edges while a maximised window has the keyboard

Date: 2026-11-09

## Status

Accepted. The user asked for it, with [ADR-0130](0130-maximise-inside-the-gaps-is-a-setting.md)'s real
maximise in mind. Builds on [ADR-0051](0051-gaps.md) and [ADR-0113](0113-the-huds-parts-ask-for-shadows.md).

## Context

With gaps on, the HUD floats the gap away from the screen's edges, each part an island with rounded
corners, an outline and a shadow. That's the look gaps are for, and it's wrong next to a window that is
maximised edge to edge (ADR-0130): the window covers the gaps, and the HUD floats on a screen with no
gaps left in it, its corners and islands cut against the window.

## Decision

- **While the window with the keyboard is maximised, and gaps are on**, the HUD is flush: no margin from
  the edge, square, no outline and no shadow, and its bars run end to end along the edges, so a split
  bar's parts are one bar, and the default L is one L, fused at the corner.
- **The reserve strips draw the bars.** A split bar already has a strip along each edge it keeps,
  anchored to the whole edge. Flush, each is as thick as what it keeps and draws the bar's background, on
  the layer under the windows (which stay clear of it by its exclusive zone) and under the parts. The
  parts draw no background of their own then, only their icons, on the strip. The whole bar, not split,
  just stops floating.
- **The colour is a setting**: `flush_black` in the HUD's config, off by default, which draws them in
  the skin's own background, as they always are. On, they are plain black, and the fills drawn on them
  (the focused tab's, the hover) are neutral greys, since the skin's are tinted and show on black. A
  checkbox on Settings' HUD page sets it. The icons and text keep the skin's colours either way.
- **"Maximised" is the compositor's word**: `maximized` in wlr-foreign-toplevel's state, which it sends
  only for a real maximise, not for a window filling inside the gaps (ADR-0093). So this needs nothing
  in the compositor, only that patina-shell's `Toplevel` carries it. Every part of a split bar binds the
  window list now, not just the tabs, since each has its margin to change; the other parts still don't
  peek out when the focus moves.
- **It's gaps of 0**: the bar keeps the gaps the config asks for and draws with none while flush, and
  works out where it sits again with them. The margin and the space reserved at the edge follow through
  the hooks the hide-and-slide already uses (`wanted_margin`, `wanted_exclusive_zone`, `wanted_size`), and
  the shadow through `shadow_hidden`, so nothing restarts.
- **Another window with the keyboard ends it**: a maximised window behind another doesn't count.

## Consequences

- With the real maximise and gaps both on, the screen looks like a maximised window on a desktop with
  a taskbar, and goes back to floating islands when it isn't.
- The edges the HUD keeps change by the gap as it goes flush, so the compositor lays the maximised
  window out again by that much, once.
- Reserves move to the bottom layer, below the windows. A floating window dragged over a flush bar's
  strip hides it, though the parts stay on top.
- On black, a skin's dark icons are hard to see; that is why it's off by default.
- The Super-key launcher path (`patina-hud --launcher`) reads the configured gaps, so a menu opened
  that way while flush is up to a gap away from where the button now is.
