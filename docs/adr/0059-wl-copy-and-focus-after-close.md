# 0059. Data-control for the clipboard, and focus back to where it came from

Date: 2026-10-17

## Status

Accepted. The user found that `pwd | wl-copy` in Teleprinter, and copying in Vim, switched to the
last window in the bar's list. The compositor's new focus log traced it.

## Context

Copying from a terminal goes through `wl-copy`. Wayland only lets a client set the clipboard
while it has the keyboard, so programs without a window use a data-control protocol instead:
wlroots' `zwlr_data_control_manager_v1`, or the standard `ext_data_control_manager_v1`. The
compositor served neither. So wl-copy fell back to opening a tiny invisible window titled
"wl-clipboard", taking the keyboard, setting the clipboard, and closing it.

Closing a window that has the keyboard focused its neighbouring tab (ADR-0027). The wl-clipboard
window was the newest, so the last tab, and its neighbour was the tab before it: the last real
window in the list, not the terminal the user was typing in. Its tab also flickered on the bar.

The same would happen with any short-lived window opened from another: a dialog, or a picker.

## Decision

- **The compositor serves both data-control protocols** (Smithay's `wlr_data_control` and
  `ext_data_control`), for every client, as wlroots desktops do. wl-copy, wl-paste and clipboard
  managers set and read the clipboard without a window. The primary selection isn't served yet.
- **Closing a window gives the keyboard back to the window it came from.** When a window takes
  the keyboard, it remembers which window had it, weakly. When it closes with the keyboard, that
  window is focused if it's still open. Otherwise the neighbouring tab is, as before. Browsers
  do the same: closing a tab you opened from another goes back to it.
- **The compositor logs focus it gives without a click or a key:** which program asked for a
  window (the bar's tabs, patina-raise), a window closing with the keyboard, and falling back
  to the top window. It's how this was found, and the lines are few.

## Consequences

- Copying in a terminal or Vim leaves the focus where it was, and no tab flickers.
- Any client can read and set the clipboard through data-control, as on sway or Hyprland.
- A dialog or other short-lived window returns you to the window you were in.
- After switching around, closing a window goes back along the chain of windows it came from,
  not always to the next tab.
