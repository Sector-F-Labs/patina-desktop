# 0121. Several monitors: extend or mirror, chosen in a popup, placed in Settings

Date: 2026-11-07

## Status

Proposed. Extending and hotplug work on the hardware backend (tested on nifl with a 21:9 monitor, unplugged and replugged); mirror, the interface, the popup and Settings are not in.

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
- **Extend, mirror, or off.** Extend maps each output at its place in the space. Mirror maps
  them all at the origin, a monitor's scale making it as wide in points as the primary, so
  windows show on all of them (a monitor of another shape is cropped). Off drives the primary
  alone. The primary is the internal panel, else the first monitor connected, and the popup is
  always on it.
- **A monitor plugged in stays off until chosen for.** It is detected and listed (`active:
  false`) but not driven: no modeset, no output. The popup offers Extend, Mirror or Do nothing,
  which leaves it off and changes nothing else. Extend and Mirror switch on every monitor
  waiting. Monitors connected at login are driven as the last choice was, unless that was off.
  Opened with no monitor waiting (Ctrl+Super+P), the third tile is Primary only.
- **A monitor runs in the mode that matches its shape.** Among the modes whose aspect ratio is
  within 6% of the panel's size in millimetres, the preferred one, else the largest. A 21:9 panel
  that accepts 4K flags it as preferred and squashes the picture. Without a size, or with no
  match, the preferred mode.
- **The tiler follows the pointer's monitor.** The compositor announces that monitor's tiling
  area and a screen pushed out past the edges another monitor touches, so the seam never tiles.
  Maximise, fill, snap and fullscreen use the window's own monitor.
- **Plug and unplug are udev connector changes.** The connectors are probed again, a monitor
  that came is driven and appended after the others (so the primary stays), one that went is
  dropped with its global and its layer surfaces closed. Windows on a removed monitor go to the
  primary, kept inside its work area; those on a monitor that shifted go with it; a fullscreen
  window on a removed monitor leaves fullscreen. The last monitor is never dropped.
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
  another (snapping to the nearest side that touches and doesn't overlap, with the edges lining
  up at the start, middle or end when close), the same choice from menus, the mode, and the
  monitors waiting. It talks to the compositor only through the bus. Scale and primary aren't
  settable yet.

## Consequences

- Every use of `State::output` is examined: layers and the bar per output (patina-shell already
  supports a surface per output), fractional scale per surface by the output it's on, maximise
  and tiling by the output a window is on, screenshots and capture by output or the whole
  desktop, lock on every output.
- Plug and unplug are hotplug events on the udev backend, so windows on an unplugged monitor
  have to be moved to one that remains.
- Mixed scales mean a window dragged across two monitors shows at one's scale on both for now.
- Only the udev backend gets several outputs; the winit and nested backends stay single.
