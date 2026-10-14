# 0015. Render nested with OpenGL ES through winit, keeping pixman as the fallback

Date: 2026-10-14

## Status

Proposed

## Context

ADR-0008 drew the nested compositor with pixman into wl_shm, because the FreeBSD test VM has no
GPU. On a machine with one (this one has an RTX 2000 Ada, the user's Lenovo an Intel GPU) that made
everything software: the compositor drew on the CPU, and apps couldn't hand over GPU buffers, so
GL apps like UnAmp and Firefox fell back to software too. The hardware backend still to come needs
a GLES renderer and linux-dmabuf anyway.

Smithay has a winit backend: a window on the parent desktop with an EGL context and its GLES
renderer, as smallvil and anvil use. Driving EGL on our own sctk window was the alternative; it's
more code for the same result.

## Decision

The compositor tries the winit backend first (`compositor/src/winit.rs`): Smithay's `GlesRenderer`,
linux-dmabuf v5 with feedback naming the GPU's render node (v3 if EGL won't name one), and the
renderer's shm formats. winit paces frames to the parent's frame callbacks: the compositor asks for
a redraw when something changed and draws on `Redraw`. If EGL can't start, or `PATINA_RENDERER=pixman`
is set, it falls back to the pixman backend of ADR-0008.

## Consequences

- On this machine, the compositor and its GL apps render on the GPU: UnAmp creates its buffers
  through linux-dmabuf.
- The GLES renderer and dmabuf import are the ones the hardware backend will use.
- No explicit sync (`linux-drm-syncobj`) yet: winit gives no DRM device to back it. NVIDIA apps may
  show tearing or stale frames nested; the hardware backend adds it.
- The compositor now links EGL (through Smithay's winit feature), on FreeBSD too.
