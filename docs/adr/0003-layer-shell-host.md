# 0003. Keep the layer-shell host separate from the bar

Date: 2026-10-14

## Status

Proposed

## Context

The Patina desktop plan needs several layer-shell pieces: the bar first, then a launcher,
notifications, an OSD and a wallpaper. Each needs the same groundwork: a layer surface per output,
output names and scale, pointer input, an egui context per surface and painting. Where that host
should live (in patina-ui, or a new patina-desktop crate) is still open.

## Decision

We will write the host as a self-contained module, `src/host.rs`, that knows nothing about bars. A
`Shell` trait says what to draw (`setup` per egui context, `ui` per output by name), and `Options`
says where (layer, edge, height). The bar is one `Shell`. The host stays in this repo until a second
desktop piece needs it, and then moves into a shared crate as Patina's ADR-0001 asks of components.

## Consequences

- The next shell piece can start from `host.rs` and `raster.rs` unchanged.
- Things only a launcher or notification would need (keyboard focus, surfaces that aren't
  full-width strips) aren't there yet; they come when that piece does.
- Until the move, there is one copy, so nothing can drift.
