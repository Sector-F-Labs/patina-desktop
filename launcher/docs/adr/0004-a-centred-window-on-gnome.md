# 0004. On GNOME, a centred window of its own, as wofi

Date: 2026-10-16

## Status

Accepted

## Context

The launcher is drawn on a wlr-layer-shell surface, anchored to the corner of the bar that
opens it. GNOME doesn't offer layer shell (its developers keep panels and menus inside GNOME
Shell), so on GNOME the launcher stopped at start with "the compositor has no layer shell". It
is still useful there, mostly for folder apps in `~/Apps`, which GNOME's app grid doesn't list,
started from a key bound in GNOME's settings or wbindkeys, the way wofi is.

GNOME doesn't let a window choose where it opens, so a launcher-sized window would land wherever
Mutter places it.

## Decision

Where there's no layer shell, patina-shell's `window_fallback` opens an ordinary window instead:
borderless, maximised and transparent, with the launcher drawn in its middle at its usual size.
It closes on Escape, a click on the clear space around it, switching to another window, or
starting an app; running it again while it's open closes it, so one key toggles it.

## Consequences

- The launcher works on GNOME and any other compositor without layer shell, centred.
- While it's open it covers the work area with a transparent window; GNOME's top bar stays.
- It doesn't sit at a bar's corner there, as there's no bar of ours to sit by.
