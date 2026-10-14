# 0014. Maximise fills the work area, and dragging restores

Date: 2026-10-14

## Status

Proposed

## Context

Floating windows (ADR-0007) need maximise: Patina's frame has a maximise button and double-click,
and foot and Firefox draw their own. Smithay's xdg-shell advertised every capability (fullscreen,
minimise, a window menu) though the compositor handled none, so clients showed buttons that did
nothing.

## Decision

- A maximised window fills the work area: the output minus the layer surfaces' exclusive zones
  (the bar). Its previous position and size are kept on the window, and unmaximising restores
  them.
- Maximised windows follow the work area when it changes: the output resizes, or the bar comes or
  goes.
- Dragging a maximised window, by its title bar or Super+drag, restores it under the pointer,
  keeping the pointer at the same fraction of its width, as GNOME and macOS do.
- Maximise and unmaximise also come from wlr-foreign-toplevel (ADR-0012), which reports the state.
- xdg-shell advertises only the maximise capability, so clients hide minimise and fullscreen
  buttons until the compositor has them.

## Consequences

- Resizing a maximised window by an edge isn't handled specially; clients usually don't offer it.
- Minimise needs somewhere for windows to go (the bar's tabs), and fullscreen needs a layer above
  the bar; both are later decisions.
