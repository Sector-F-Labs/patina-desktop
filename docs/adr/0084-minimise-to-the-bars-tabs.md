# 0084. Minimised windows leave the space and keep their tabs

Date: 2026-10-24

## Status

Accepted

## Context

ADR-0014 left minimise for later, because a window needs somewhere to go: xdg-shell advertised
only maximise. Patina apps showed a minimise button that did nothing anyway, since winit drops
`wm_capabilities` and Patina guesses by compositor (`can_minimise_and_maximise`), and the
compositor's frames for X11 windows hid theirs (ADR-0077). ADR-0044 had a Wine game that minimises
itself on Alt+Tab told it's iconic while it stayed on screen, because there was nowhere to put it.

The bar's tabs are the one window list (ADR-0027), and a window's tab is already there whether
the window is shown or not. So that's where a minimised window goes, with no dock or shelf of its
own. Two ways of hiding it were considered. One is to keep it in Smithay's `Space` and skip it
everywhere it's drawn, hit and focused. That is a check in every backend and in every walk over
the space, and the first one forgotten draws a hidden window. The other is to take it out of the
space and keep it aside, so everything that walks the space passes it by without changes. The
second was chosen.

## Decision

- Minimising takes a window out of the space and keeps it in `State::minimized` with its place
  (`minimize.rs`). If it had the keyboard, the window then on top gets it.
- It stays open: `window_for`, the foreign toplevels (with their `minimized` state), Alt+Tab's
  order and Super+Q still see it.
- Focusing a minimised window puts it back where it was, on top. This is in `State::focus`, so
  its tab, Alt+Tab, Super+1–9 and patina-raise all restore it, with nothing of their own.
- Closing a window doesn't give the keyboard to a minimised one: closing passes minimised windows
  over when it picks the next tab.
- A window is minimised by xdg `set_minimized` (the capability is now advertised), by
  foreign-toplevel `set_minimized`, by an X11 request, by the minimise button on the frames
  drawn for X11 windows, and by the `minimise` key, Super+M by default (Cmd+M on macOS).
- An X window minimised is told it's iconic, as before (ADR-0044), and is restored when focused.
  A Wine game that minimises itself on Alt+Tab now leaves the screen too.

## Consequences

- Patina apps' minimise buttons work, and other apps show theirs.
- Super+M no longer reaches apps as Ctrl+M (ADR-0067). `[keys] minimise = ""` gives it back.
- Minimised windows get no frame callbacks, so they stop drawing until they're restored, as on
  other desktops.
- The bar doesn't yet show a minimised tab differently, and clicking the shown tab doesn't
  minimise it. Both need the `minimized` state and a minimise request in patina-shell's
  toplevels, and a look for it in Patina's `TabStrip`.
- A maximised or tiled window is laid out again when it's restored, in case the work area
  changed while it was minimised.
