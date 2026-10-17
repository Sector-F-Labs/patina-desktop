# 0027. The bar's tabs are the one window list: Alt+Tab and closing follow them

Date: 2026-10-15

## Status

Accepted. Closing a window now gives the keyboard back to the window it came from first
([ADR-0059](0059-wl-copy-and-focus-after-close.md)).

## Context

The bar shows the open windows as tabs in the order they opened (ADR-0012), and Super+1 to 9 pick
them. Closing a window focused whatever was on top of the stack, and Alt+Tab did nothing. Most
desktops add an Alt+Tab switcher, an overlay with its own most-recently-used order. The user's
principle for the desktop's UI is not to repeat ourselves: a switcher would be a second list of the
same windows, in a second order, so Alt+Tab should work as part of the bar.

## Decision

- **The bar's tab order is the window order.** Alt+Tab activates the next tab to the right, wrapping
  round; Alt+Shift+Tab the one to the left. There's no overlay: the bar's highlighted tab shows where
  you are.
- **Closing or quitting focuses the neighbouring tab**, as closing a browser tab does: the next one,
  or the one before if it was the last. Closing a window in the background leaves focus alone.
- The principle for the desktop as a whole: a piece of UI isn't repeated in a second form. Where a
  feature already has a home (the bar for windows, the tray for status), it grows there.

## Consequences

- One mental model: what's in the bar, in its order, is what keys and closing move through.
- Alt+Tab doesn't flip between the two most recent windows, as it does elsewhere; with many windows,
  getting back to one can take several presses (Super+1 to 9 jump directly).
