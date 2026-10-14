# 0011. Move the layer-shell host into patina-shell

Date: 2026-10-14

## Status

Proposed

## Context

ADR-0003 kept the layer-shell host in the bar until a second shell piece needed it. The launcher
is that piece. It also needs things the bar didn't: a centred surface rather than a strip, one
surface instead of one per output, the keyboard (with key repeat), and a way to end the program.

## Decision

The host (`host.rs`) and the software painter (`raster.rs`) move into a workspace library,
`shell/` (`patina-shell`). `Options` now takes any anchor and size, an exclusive zone, keyboard
interactivity and whether there is a surface per output. Keys reach egui as key and text events,
and focus as `WindowFocused`. A `Shell` can say it's `finished`. It re-exports calloop, so the
pieces don't depend on sctk directly.

## Consequences

- The bar and the launcher share one copy, and later pieces (notifications, an OSD) start from it.
- It stays in this workspace, not in patina-ui: it's desktop plumbing, not app components.
