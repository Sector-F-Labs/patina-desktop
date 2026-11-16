# 0146. Dragging with Super held tiles to the half the pointer is in

Date: 2026-11-16

## Status

Accepted. Builds on [0049](0049-tiling-by-dragging-decided-over-d-bus.md) and
[0053](0053-tiling-decided-by-patina-tiler.md).

## Context

A dragged window tiles only when the pointer reaches within 4 px of the left or right edge (or
the top). That is exact but takes a long, deliberate drag. Someone already holding Super to move
a window has said they mean to tile it, and shouldn't need to reach the edge.

## Decision

- **The compositor says whether Super is held in a drag**: a new signal `DragSuper(t id, b held)`
  on `com.sectorflabs.Patina.Tiling`, sent as a drag starts with Super held and whenever Super is
  pressed or let go during it. A new signal, so tilers written before it go on working.
- **patina-tiler tiles to the half the pointer is in while it's held** (`policy::zone_held`): the
  left half of the screen is Left, the right half Right, whatever the distance from the edge, with
  the preview shown at once. The top edge still fills. Letting Super go in the middle of a drag
  goes back to the edges, and the preview follows.
- The drop uses what the preview shows, as before.

## Consequences

- A Super drag always ends tiled, unless Super is let go first, and the preview says where.
- Moving a window with Super and leaving it floating now means letting Super go before the
  button.
- Other tilers get `DragSuper` and may use it, or ignore it.
