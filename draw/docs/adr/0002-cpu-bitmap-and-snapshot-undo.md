# 0002. Draw on a CPU bitmap, undo by snapshots

Date: 2026-10-28

## Status

Proposed

## Context

Draw is a Paint-style editor: pixels, not vectors. The tools (pencil, brush, eraser, fill, line,
rectangle, ellipse) each change pixels, and a shape has to be stretchable before it is let go.
Options: keep a vector scene and rasterise it (Inkscape-like), draw with egui's painter into a
texture on the GPU, or keep our own RGBA bitmap and upload it as a texture to show it.

## Decision

The picture is a `Bitmap` of RGBA pixels (`src/bitmap.rs`) that the tools change directly, with
no egui in the drawing. egui only shows it, as a texture sampled nearest-neighbour so zoomed
pixels stay square. Undo keeps a whole-bitmap snapshot per step, 50 deep. A shape is redrawn from
the snapshot taken at the press on every drag, so nothing needs erasing.

## Consequences

- Every tool is plain Rust and tested without a window.
- Memory for undo is steps × picture size: an 8000 × 6000 picture is 192 MB a step, so a very
  large picture would want diffs or tiles. Fine for the sketches and screenshots Draw is for.
- No layers, selections or antialiasing yet. Adding them changes `Bitmap` and `Document`, not
  the UI.
- Pictures are flattened onto white on opening, so there is no transparency to track.
