# 0007. Floating windows first

Date: 2026-10-14

## Status

Accepted

## Context

The "Patina desktop" note left open whether the desktop tiles or floats. Patina apps draw their
own frame and header bar, which dwl's tiling made mostly cosmetic. Tiling could still come later
as a mode.

## Decision

patina-compositor floats windows, as GNOME and macOS do. A new window is placed once, cascading
from the top left of the area the layer surfaces leave free, and keeps its own size. A click
focuses and raises a window. No server-side decorations are offered, so clients draw their own.

## Consequences

- Patina's frame is the window's only frame; move and resize requests are next to honour.
- No tiling layouts yet; a tiled mode would be a new ADR.
