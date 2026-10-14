# 0023. Super with the arrows or vim keys maximises, restores and tiles half-screen

Date: 2026-10-15

## Status

Accepted

## Context

Windows float (ADR-0007) and could be maximised (ADR-0014), but only with the mouse. The user asked
for Ubuntu's keys, Super with the arrows, and for vim's h/j/k/l to do the same.

## Decision

On the focused window:

- **Super+Up / Super+K** maximises.
- **Super+Down / Super+J** restores, from a maximise or a tile.
- **Super+Left / Super+H** and **Super+Right / Super+L** fill that half of the work area (the left
  half takes an odd pixel); from the other half they restore instead, as on Ubuntu.

Maximising becomes one case of a window's placement (maximised, left or right half), remembered
with where the window was before, so everything maximise already did applies to tiles: following
the work area when it changes, keeping the stacking order, and restoring under the pointer when
dragged. Tiled windows get xdg-shell's tiled states on the sides that touch the screen's edges, so
apps can square those corners.

## Consequences

- Two windows side by side with two keys, without a tiling layout.
- Quarter tiles, tiling by dragging to an edge, and resizing the split between two tiled windows
  are not done.
- Super+H, J, K and L are no longer available to apps or bindings; Super+L isn't a lock-screen key
  here.
