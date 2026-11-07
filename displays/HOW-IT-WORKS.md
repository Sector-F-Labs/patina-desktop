# How several monitors fit into the desktop

Plug in a second monitor and nothing lights up by itself. A moment later a pill appears on the
laptop's screen asking whether to extend the desktop onto it, mirror it, or do nothing; extending
onto a monitor the desktop hasn't seen before opens Settings, to say which side of the laptop it
sits on. This page explains how that is split between the compositor, a small popup and
Settings, and why. The README next to it covers running and testing; the decisions are in
ADR-0121.

## The idea in one paragraph

Only the compositor can drive a monitor, so it keeps everything that is a fact about them: which
are connected, which are driven, who each one is, where each one sits, and the mode. But *what
the user wants* is asked for in two other places: a popup that comes when a monitor does, and a
Settings page for arranging them. Both are clients of one bus name,
`com.sectorflabs.Patina.Displays`, and neither draws a monitor, so either can be rebuilt and
restarted without touching the screen. The one rule that shapes the rest: **a monitor plugged in
while the desktop runs stays off until someone chooses for it.**

## The picture

<img src="images/architecture.svg" alt="The compositor, on the left, keeps the monitors in udev.rs, who each is in edid.rs, the mode and saved places in displays.rs, positions in layout.rs, and serves the bus from displays/dbus.rs; the places are saved in displays.toml. The session bus in the middle carries List, Mode, SetMode and SetPlace in, and Changed and MonitorAdded out. On the right, patina-displays and Settings' Displays page use the shared client in patina_system::displays; the page's drag is display_layout.rs. Below, a monitor's EDID makes its id, and a monitor is known when its id is in displays.toml.">

## The sequence, in time

<img src="images/sequence.svg" alt="A monitor is plugged in and left idle by the compositor. A moment later the compositor starts patina-displays, which asks for the list. Choosing Extend sends SetMode and the compositor drives the monitor. Because it was never placed, the popup opens Settings' Displays page, where dragging the monitor sends SetPlace; the compositor saves it and answers Changed. Next time the same monitor is known.">

### Why the popup waits, and asks again

Bringing a monitor up (a mode set, a new output, the first frames) keeps the compositor's event
loop busy for over a second, and the bus is answered from that loop. A popup started at once
asks `List()` into the middle of it and times out, so the compositor starts it 1.5 seconds after
the plug-in, and the popup asks again for a few seconds before giving up. The tiler fails the
same way at that moment and recovers on its own.

## What decides what

| Event | What happens |
|-------|--------------|
| A connector changes (udev) | `UdevData::rescan` looks again. A monitor that went is dropped; one that came is left **idle** (connected, listed with `active = false`, not driven). The first monitor of an empty desktop is the exception: it is driven, as the primary. |
| Ctrl+Super+P, or the plug-in's timer | The compositor starts `patina-displays`, always on the primary monitor (`new_layer_surface`). |
| Extend or Mirror chosen | `set_display_mode` drives every idle monitor, `outputs_changed` lays them all out, the mode is saved. |
| Do nothing (a monitor waiting) | The popup closes without calling anything; the monitor stays off and nothing else changes. |
| Primary only (nothing waiting) | `Mode::Off`: every monitor but the primary stops being driven and goes idle. |
| `SetPlace(id, to, side, offset)` | The place is saved by `id` and the monitors are laid out again; windows move with their monitor. |
| Extend onto a monitor never placed | The popup opens Settings at the Displays page (`--page displays`). |
| A monitor unplugged | Its output and its bar and wallpaper go. Windows on it move to the primary, kept inside its work area; a fullscreen one leaves fullscreen. |

## Who a monitor is

Ports change: the same monitor is DP-3 on one day and DP-2 on the next. So a monitor is known by
what it says about itself, the maker, model and serial in its EDID (`edid.rs`), as
`GSM-772E-0005F79F`. That id is what `displays.toml` is keyed by, and a monitor whose id is in it
is **known**: it has been placed before. A monitor whose EDID has no serial gets its connector
added, so two of one model are told apart. The panel inside a laptop has no model name at all,
only a product code, so it is shown as "Built-in display".

The compositor never reads the id back out of the file as a layout: `displays.toml` holds the
*choice* (this monitor is above that one, 0 points along the edge); where that puts it, in
points, is `layout.rs`, from the monitors' current sizes and scales.

## The three modes

| Mode | What the compositor does |
|------|--------------------------|
| **Extend** | Each output is mapped into the space at the position `layout::arrange` gives it. A monitor with no saved place goes right of the one before. The pointer crosses where monitors touch and is held at the edges where they don't. Maximise, fill, snap and fullscreen use the window's own monitor. |
| **Mirror** | Every output is mapped at the origin. A monitor's scale is set so it is as many points wide as the primary, so the same desktop fits it; one of another shape is cropped. The pointer stays on the primary. |
| **Off** | Only the primary is driven. The others are idle, exactly as a monitor just plugged in is. |

## What the compositor keeps

| Job | Why it isn't a client's |
|-----|-------------------------|
| Drive the monitors (`udev.rs`) | Only the process that owns the GPU can. Each driven monitor is a `Head` with its own DRM compositor, vblank and gamma ramp. |
| Choose each monitor's mode | A monitor that takes modes of another shape squashes them and often prefers one: `pick_mode` keeps those whose shape matches the panel's size in millimetres. |
| Place windows when monitors change | It owns the windows, and a removed monitor's must move before the next frame. |
| Keep the choice (`displays.toml`) | So it is applied at login, before any client runs. |
| Tell the tiler which monitor it is working on | The tiler asks `WorkArea()` and `Screen()`; the answers follow the pointer, with its edge zones pushed out past edges another monitor touches, so dragging across the seam never tiles. |

## What the clients keep

| Job | Where |
|-----|-------|
| Offer the choice | `patina-displays`: the third tile is **Do nothing** when a monitor is waiting and **Primary only** when none is. |
| Draw the monitors to scale, and where a drag lands | Settings' `displays.rs`, with the snapping in `display_layout.rs`: the nearest side of another monitor that touches along its edge and overlaps none, with the edges lined up at the start, middle or end when close. Pure arithmetic, with tests. |
| Say what is new | A monitor not known is marked, and the page says to drag it to where it sits. |

## When it's not running

- **Nested** in another desktop (or `make run`), there is one output, the window, and the
  service lists it as the only monitor; the popup has nothing to offer and the page says so.
- **Without the compositor on the bus**, the popup exits with an error and the page says
  displays are the compositor's.
- **No displays.toml** is every monitor new: the first extend sends you to Settings once for each.

## Things worth knowing

- **Never read the pointer's location from inside a pointer grab.** The grab holds the pointer's
  lock, so asking the handle for it blocks the whole compositor forever. `drag_moved` is given
  the location instead.
- **A degraded DisplayPort link looks like a smaller monitor.** After a compositor was killed
  mid-frame, the monitor offered nothing above 1080p, with the same EDID. Writing `detect` to
  `/sys/class/drm/<connector>/status` makes the kernel probe it again and the full list returns;
  a plain restart does not.
- **A gamma ramp left warm is not neutral.** Night Light was warming when the compositor was
  killed, and every later instance read the red ramp as "the original". A ramp that isn't close to
  a straight line is replaced.
- **An output's name outlives its surface.** Anything cached by output name (the wallpaper's
  textures were) is wrong after the output is switched off and on, because the new surface has a
  new egui context that knows none of the old textures.

## The files

```
 patina-desktop/
 ├── compositor/src/
 │   ├── udev.rs            Head per driven monitor, idle ones, rescan, hotplug, pick_mode
 │   ├── edid.rs            a monitor's maker, model and serial, and its id
 │   ├── displays.rs        the mode, the saved places, and what the bus asks for
 │   ├── displays/dbus.rs   com.sectorflabs.Patina.Displays
 │   ├── layout.rs          places to positions, the pointer's clamp, View
 │   └── state.rs           outputs_changed: advertise, lay out, move the windows
 ├── system/src/displays.rs the client of the bus: list, mode, set_mode, set_place
 ├── displays/src/          the popup: main.rs, hud.rs
 └── settings/src/
     ├── displays.rs        the Displays page: canvas, menus, monitors waiting
     └── display_layout.rs  where a drag lands, as pure functions with their tests
```

The diagrams in `images/` are hand-drawn SVG, and follow the viewer's light or dark theme.

## Limits, as they stand

- Screenshots, screen capture, touch and the lock are the primary's: the lock leaves the other
  screens black.
- Mirror and Primary only apply to every monitor together, not one at a time.
- No per-monitor scale or resolution, and the primary can't be chosen.
- Mirroring a monitor of another shape crops it rather than letterboxing.
- Monitors are only found on the GPU the compositor drives.
