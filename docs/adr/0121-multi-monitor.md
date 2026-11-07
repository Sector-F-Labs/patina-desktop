# 0121. Several monitors: extend or mirror, chosen in a popup, placed in Settings

Date: 2026-11-07

## Status

Proposed. Extending works on the hardware backend for the monitors connected at start-up (tested on nifl with a 21:9 monitor); hotplug, mirror, the interface, the popup and Settings are not in.

## Context

The compositor has one output end to end: `State::output`, one DRM surface and `DrmCompositor`
in `udev.rs`, one layer map (so one bar), one target for screenshots and capture, and a pointer
clamped to one screen. A second monitor is ignored.

The user wants the experience of the screenshot pill (ADR-0091): a popup listing the monitors
and what to do with them, mirror or extend. Choosing extend for a monitor never seen before
goes straight to Settings, to say where it sits (above, below, left or right of another).

## Decision

- **The compositor drives every connected monitor.** `State::outputs` lists them; `State::output`
  stays as the primary (the internal panel, else the first), so the code that wants one screen
  keeps working. Each monitor has its own `DrmCompositor`, gamma ramp and frame state, and each
  is drawn with the desktop offset by its place in the space.
- **Extend and mirror are the two modes.** Extend maps each output at its place in the space.
  Mirror maps them all at the origin, at the primary's size, so windows show on all of them.
  Only one monitor can be "off" besides: a monitor turned off is unmapped and its CRTC cleared.
- **A monitor runs in the mode that matches its shape.** Among the modes whose aspect ratio is
  within 6% of the panel's size in millimetres, the preferred one, else the largest. A 21:9 panel
  that accepts 4K flags it as preferred and squashes the picture. Without a size, or with no
  match, the preferred mode.
- **The tiler follows the pointer's monitor.** The compositor announces that monitor's tiling
  area and a screen pushed out past the edges another monitor touches, so the seam never tiles.
  Maximise, fill, snap and fullscreen use the window's own monitor.
- **Placement is `layout::Place`**: a side of another monitor plus an offset along the edge,
  saved by monitor id (EDID make, model and serial, else the connector name) in the compositor's
  config. A monitor not in the file is *new*, and lands right of the one before it until placed.
  The pointer crosses where monitors touch and is held at the edges where they don't.
- **A D-Bus interface, `com.sectorflabs.Patina.Displays`**, lists monitors (id, name, size,
  scale, place, whether seen before), sets the mode (`extend`, `mirror`) and a monitor's place,
  scale and on/off, and signals `Changed` and `MonitorAdded`. As with Night Light, the compositor
  keeps the state and Settings and the popup are clients.
- **The popup is `patina-displays`**, a patina-shell pill in the screenshot's style, opened by
  `Super+P` and by the compositor when a monitor is plugged in. Mirror and Extend apply at
  once; Extend with a new monitor then opens Settings' Displays page on it. A monitor seen
  before gets its saved place back with no further question.
- **Settings gets a Displays page** with the monitors drawn to scale, dragged to a side of
  another, with scale and primary.

## Consequences

- Every use of `State::output` is examined: layers and the bar per output (patina-shell already
  supports a surface per output), fractional scale per surface by the output it's on, maximise
  and tiling by the output a window is on, screenshots and capture by output or the whole
  desktop, lock on every output.
- Plug and unplug are hotplug events on the udev backend, so windows on an unplugged monitor
  have to be moved to one that remains.
- Mixed scales mean a window dragged across two monitors shows at one's scale on both for now.
- Only the udev backend gets several outputs; the winit and nested backends stay single.
