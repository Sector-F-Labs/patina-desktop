# 0016. Run on the hardware with DRM/KMS, GBM, libinput and libseat

Date: 2026-10-14

## Status

Proposed

## Context

To be a login session (docs/running-as-a-session.md) the compositor has to drive the machine itself
rather than run as a window on another desktop. The user wants to pull the repo on an Intel Lenovo
laptop and `make install-session`; this machine has an NVIDIA RTX 2000 Ada. Smithay provides the
pieces; anvil wires them for many GPUs and monitors through `GpuManager` and `DrmOutputManager`,
which is more than a first version needs.

## Decision

`compositor/src/udev.rs`, chosen when `WAYLAND_DISPLAY` is unset:

- **Session:** libseat (through logind or seatd) opens the devices, and on a console switch the
  compositor gives them up (libinput suspended, DRM paused) and takes them back, resetting the
  DRM state and redrawing.
- **GPU:** the seat's primary GPU, through `DrmDevice`, GBM and EGL, rendered with the same
  `GlesRenderer` and linux-dmabuf as the nested GL backend (ADR-0015). One `DrmCompositor` does
  mode-setting, page flips and plane assignment, paced by vblank; when a frame has no damage, the
  next is held back 16 ms instead.
- **Monitor:** one: the internal panel if connected (eDP, LVDS, DSI), otherwise the first
  connected monitor, at its preferred mode. The scale is 2 for dense panels at least 800 points
  tall at 2x, 1.5 from 140 dpi, else 1; `PATINA_SCALE` overrides it.
- **Input:** libinput on the seat, with tap-to-click turned on for touchpads.
- **Cursor:** the client's cursor surface, or the cursor theme's arrow (`XCURSOR_THEME`,
  `XCURSOR_SIZE`), or a drawn one where there's no theme.
- **Keys the compositor keeps:** Ctrl+Alt+F1–F12 switch consoles; Ctrl+Alt+Backspace quits, the
  way out of a session that has gone wrong. Input handling is shared with the nested backends
  (`input.rs`), and linux-dmabuf with the GL one (`dmabuf.rs`).

## Consequences

- It runs where GBM and EGL do: Mesa (Intel, AMD) and NVIDIA's driver from 495 on.
- One monitor: others stay dark, and hotplug isn't handled. Several monitors need the compositor to
  stop assuming one output (layer maps, maximise, the bar's placement), a refactor of its own.
- No explicit sync (`linux-drm-syncobj`) yet, which NVIDIA clients want; no lid, brightness or
  volume keys; touchpad settings beyond tap aren't configurable.
- It can only be tested where the compositor can take the GPU: a text console, a login session, or
  a Linux VM with a virtio GPU. Run inside GNOME, libseat can't open the card GNOME holds, so it
  can't disturb the desktop.
