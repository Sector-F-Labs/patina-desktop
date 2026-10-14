# 0008. Run nested, drawn in software with pixman

Date: 2026-10-14

## Status

Proposed

## Context

Until it runs on real hardware, the compositor needs a backend that runs as a window on another
desktop: the user's GNOME, or the test VM's dwl. Smithay's winit backend does this, but it renders
with OpenGL ES through EGL, which the VM doesn't have (no GPU driver, and its wlroots draws with
pixman). Smithay also has a pixman renderer, which needs no GPU.

## Decision

We will run nested through our own small backend (`compositor/src/nested.rs`): an sctk xdg window
on the parent desktop, the output rendered with Smithay's `PixmanRenderer` into a pixman image,
copied into a wl_shm buffer and committed on the parent's frame callbacks. The window's pointer
and keyboard (evdev codes) drive the compositor's seat. It is the same software approach as the
bar's (ADR-0002).

## Consequences

- It runs anywhere wl_shm does: GNOME, the VM, any wlroots desktop.
- Every frame is a CPU render plus a copy; fine for a 1280x800 desktop with damage tracking, not
  for games or video.
- On GNOME the window has no title bar (Mutter does no server-side decorations); move it with
  Super+drag.
- No HiDPI yet: the output is scale 1.
- A real-hardware backend (DRM/KMS, libinput, a GPU renderer) is a separate, later decision.
