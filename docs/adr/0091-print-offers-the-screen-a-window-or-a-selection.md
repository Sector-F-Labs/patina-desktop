# 0091. The Print key offers the screen, a window or a selection

Date: 2026-10-25

## Status

Proposed. The user asked for the Print key, not the Mac's shortcuts, to open a HUD like
dictation's with the choice of the full screen, a window, or a part of the desktop.

## Context

Print took the whole desktop outright (ADR-0034), the same as Super+Shift+3, and there was no
way to take a part of the screen at all. macOS has Cmd+Shift+5 for this: a bar at the bottom of
the screen with the same choices. The Mac's own keys (Super+Shift+3 and 4) should keep doing
what they do at once.

The picture is the compositor's to take: it alone can draw the desktop (there's no
wlr-screencopy), and it alone knows where the pointer is and which window was focused. A HUD with
labels and icons is egui, and the pieces that draw egui are patina-shell programs (ADR-0011),
like dictation's overlay (ADR-0073).

## Decision

- **A new action, `screenshot_menu`**, on Print and Super+Shift+5 by default; `screenshot` keeps
  Super+Shift+3 alone. Like the other screenshot keys it does nothing under a fullscreen window
  or the lock.
- **`patina-screenshot`** (`screenshot/`) is the HUD: a pill centred at the bottom of the screen,
  in the bar's skin, on the overlay layer, with a close button, then Screen, Window and
  Selection, each an icon over its name. It takes the keyboard: arrows and Tab move, Enter or
  Space or 1 to 3 choose, Escape or the keyboard going elsewhere closes it. It prints the word
  for the choice (`screen`, `window`, `selection`), or nothing, and exits.
- **The compositor starts it and reads its stdout** on a thread, as the bar reads patina-menu's
  items (ADR-0056), and acts once it has exited, so the pill is gone before anything is drawn;
  screenshots also leave out its layer by namespace, in case it's still mapped. No D-Bus
  interface: the choice is one line, to the one program that asked.
- **The window taken is the one focused when the key was pressed**, kept by the compositor,
  since the pill takes the keyboard from it. Without one, `--no-window` greys Window out; one
  closed meanwhile takes nothing.
- **A selection is the compositor's** (`screenshot/select.rs`): the screen dims under the
  crosshair, the part dragged out shows through with a one-pixel white edge, and letting go takes
  the desktop cut to it. Meanwhile the pointer and the clicks reach no window. Escape, another
  button, or a click without a drag takes nothing. The dim and edge are solid-colour elements
  drawn by every backend beside the flash.

## Consequences

- Print takes one more key press for the whole screen (Print, then Enter), and Super+Shift+3 is
  still one.
- A part of the screen can be taken, anywhere a window or layer is.
- A process and a Wayland connection start at each Print, as the dictation overlay does.
- A selection is one output's, as the compositor has one; it isn't frozen while it's made.
