# 0049. Tiling by dragging, as on Ubuntu, with the decisions open to any app over D-Bus

Date: 2026-10-17

## Status

Proposed

## Context

Windows could already be tiled from the keyboard: Super+Left/Right for halves, Super+Up to
maximise (ADR-0014, ADR-0023). Dragging a tiled window restored it under the pointer. But
dragging a window against a screen edge did nothing. On Ubuntu that's how most people tile:
drag the window to the left or right edge to fill that half, or to the top to maximise, with a
preview of where it will go.

The user asked for Ubuntu's behaviour, with one condition: it should be managed over D-Bus, so
other applications can decide how windows tile and implement it. A tiling policy isn't one fixed
thing. People want thirds, quarters, ultrawide columns, or i3-like layouts, and that shouldn't
mean forking the compositor.

Options considered:

- **The policy only in the compositor, with a config file.** This is quick, but every new layout
  means compositor code, and it's not what was asked.
- **Policy only outside, in a default tiler process the compositor starts.** This is clean, since
  the compositor would hold no policy. But a crash or a slow start would leave the desktop with
  no tiling at all, and every drag would need a round trip before the preview could show.
- **The compositor's own policy as the default, which a tiler on the bus can replace.** Tiling
  always works, and replacing it is just running a program.

Who the tiler is should follow the bus's own rules, not a registry inside the compositor. Owning
a well-known name gives that for free: replacing the tiler, queueing, and letting go when the
owner leaves the bus or crashes.

## Decision

- **Dragging tiles, as on Ubuntu.** A window dragged (by its title bar, or with Super) so that
  the pointer reaches within 4 px of the screen's left or right edge shows a translucent preview
  of that half. Let go there, and the window is tiled to it. The top edge does the same with the
  whole work area, which maximises the window. The sides win in the top corners. Restoring by
  dragging works as before.
- **The compositor serves `com.sectorflabs.Patina.Tiling`** at `/com/sectorflabs/Patina/Tiling`
  (`compositor/src/tiling/dbus.rs`), nested too:
  - Signals: `DragStarted(t id)`, `DragMotion(t id, d x, d y)`, `DragEnded(t id, d x, d y)`
    (the pointer, in logical pixels), `SnapRequested(t id, s direction)` and
    `WorkAreaChanged(i x, i y, i w, i h)`. Motions are coalesced to the latest when the bus
    falls behind.
  - Methods: `Windows()`, which returns (id, app id, title, placement, rect) per window;
    `WorkArea()`; `Place(id, x, y, w, h)`; `Restore(id)`; `ShowPreview(x, y, w, h)`; and
    `HidePreview()`.
- **Whoever owns `com.sectorflabs.Patina.Tiler` decides.** While the name has an owner, the
  compositor still reports every drag but doesn't tile on a drop or show its own preview.
  Super+arrows become `SnapRequested` instead of snapping. When the owner leaves the bus, the
  compositor decides again. The drag signals go out either way, so other apps can watch.
- **Placement gains `Rect`**: a window placed by a tiler can be anywhere, not just a half. The
  xdg tiled states now come from which sides of the window touch the work area. That's correct
  for halves, thirds and quarters alike. `Place` with the whole work area maximises the window.
- **The client side is `patina_system::tiling`** (`Tiling`, `Event`, `claim`).
  `cargo run -p patina-system --example thirds` is a whole tiler in about a hundred lines: the
  edges give thirds, Super+Left/Right step along them.

## Consequences

- Ubuntu's drag-to-tile works with nothing else running, and replacing it is just running a
  program. No compositor change and no config are needed.
- Each window gets a number that is stable while it's open. This is the first time windows are
  named on the bus. It may be worth making the bar's tabs and Settings use the same numbers
  later.
- The preview is drawn over the dragged window, not under it as GNOME draws it. That would need
  the dragged window drawn as an overlay of its own.
- X11 windows aren't listed and can't be placed yet, because tiling goes through xdg-toplevel
  configures. They need the same through the X11 window manager.
- A tiler sees drags but not new windows. An automatic tiler (i3-like) also needs
  `WindowOpened` and `WindowClosed`, and a way to place a window before it's first shown. Add
  them when one is written.
- The compositor holds the window list while a method waits for it: a `Windows()` call blocks
  zbus's thread until the event loop answers, for at most a second, as `RestartDesktop` already
  does.
