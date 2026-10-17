# 0054. Dragging the line between tiled windows resizes both

Date: 2026-10-17

## Status

Accepted. The user asked for it: with windows tiled side by side, putting the pointer between
them and dragging left or right should grow one as it shrinks the other, keeping the gap
between them (or none).

## Context

Tiled halves (ADR-0049, ADR-0053) were always even. Changing that meant restoring a window and
resizing it by hand, after which it was no longer tiled. GNOME, Windows Snap and most tiling
window managers let the line between tiled windows be dragged.

ADR-0053 moved tiling's decisions into patina-tiler, but dragging a line is direct manipulation,
like resizing a window by its edge:

- it needs the pointer's cursor;
- it needs a pointer grab;
- each window has to follow the pointer on every motion.

Doing that over the bus would put a round trip in every motion and mean the tiler grabbing the
pointer, which a D-Bus client can't do.

## Decision

- **The compositor handles the drag** (`compositor/src/split.rs`). A line is where two windows
  the tiler placed (`Placement::Rect`) meet side by side, exactly the gap apart (or touching,
  without gaps). The line covers the gap, plus 4 px either side, and spans where both windows
  are.
  - **Hovering:** over a line, the pointer shows the left-right arrow and no window gets it,
    unless a window not on the line, a top or overlay layer, or a fullscreen window is on top.
  - **Dragging:** pressing the left button there grabs the line. Every tiled window whose edge
    is on it moves too, so two stacked windows on one side both follow. The windows on the left
    grow as those on the right shrink, live, with the xdg `resizing` state. No window goes below
    120 px or the app's own minimum width.
- **The tiler hears the result.** On release, a new `Resized(id, x, y, w, h)` signal goes out for
  each window that moved. patina-tiler keeps a `split` (where the columns meet, as a share of the
  room beside the gap). The split is kept while any half stays tiled:
  - a window then tiled to the other side takes the rest;
  - a change of work area keeps the same split;
  - once no half is tiled, it's even halves again.
- **Only vertical lines.** Dragging up and down between stacked windows was not asked for, and no
  tiler stacks windows yet.

## Consequences

- Tiled windows can be any width, without leaving tiling.
- Finding the line is the compositor's own rule: tiled windows exactly a gap apart. A tiler that
  leaves a different gap gets no line, and one that puts windows a gap apart gets one whether
  it wants it or not.
- While the line moves, the windows on the right are moved before their apps have drawn at the
  new size, so a frame may show them overlapping or apart.
- The arrow shows only on the hardware, where the compositor draws the pointer. Nested, the
  parent desktop's cursor stays, though dragging works.
- A tiler that ignores `Resized`, like the thirds example, has its split undone when it next tiles.
