# 0002. Draw egui in software into wl_shm buffers

Date: 2026-10-14

## Status

Proposed

## Context

Without eframe, the bar has to paint egui's tessellated meshes itself. The options were:

- `egui_glow` over EGL, or `egui-wgpu`: the usual GPU painters, but they need a working EGL or
  Vulkan driver. The FreeBSD test VM has none (QEMU's virtual GPUs have no FreeBSD driver), and
  its wlroots renders with pixman, so GL there would be llvmpipe at best.
- `egui_software_backend`: a CPU painter, but at 0.0.3 it targets egui 0.34, and Patina is on 0.35.
- A small CPU painter of our own.

A bar is a thin strip, about 1280x26 pixels per output, that redraws on a status change and once a
second. A triangle rasteriser for that is about 150 lines.

## Decision

We will rasterise egui's meshes on the CPU (`src/raster.rs`): textured, vertex-coloured triangles
with a top-left fill rule, bilinear sampling, premultiplied-alpha blending in gamma space, as
egui's GL painter does, into premultiplied ARGB8888 wl_shm buffers.

## Consequences

- No GL, EGL or Vulkan dependency: the bar runs on any compositor with wl_shm, including the
  headless, software-rendered one in the VM.
- Painting is our code to keep correct; the tests in `raster.rs` cover fills, shared edges,
  clipping, scale, text and blending.
- Large or animated surfaces (a full-screen launcher with effects, a wallpaper) may be too slow on
  the CPU. Revisit for them, keeping the host's `Shell` interface, with a GPU painter.
