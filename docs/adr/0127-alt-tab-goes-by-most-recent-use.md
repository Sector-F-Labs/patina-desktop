# 0127. Alt+Tab goes by most recent use

Date: 2026-11-08

## Status

Accepted

Changes how Alt+Tab picks the window in [0027](0027-bar-tabs-are-the-window-list.md); closing a window still follows [0059](0059-wl-copy-and-focus-after-close.md).

## Context

Alt+Tab stepped to the next tab of the bar (ADR-0027). That was chosen on purpose, for simplicity:
no switcher of its own, nothing new drawn on screen, the bar's window list that is already there
doing the job, and no hidden most-recently-used order. On the desk it felt wrong: a quick Alt+Tab is expected to swap with the
window used just before, and another to swap back, as on Windows, macOS's Cmd+Tab, GNOME and KDE.
Going by tab order, two windows were only swapped by luck of where their tabs sat, and with more
than two a quick Alt+Tab went to whichever tab came next.

## Decision

- **The compositor keeps the order windows were last focused in** (`tabs::Recent`), updated
  whenever a window takes the keyboard, by Alt+Tab, a click, a tab or a new window.
- **A quick Alt+Tab goes to the window used before this one**, and the next quick one goes back.
- **Holding Alt and pressing Tab again goes further back** through that order, and wraps to where
  it started. **Alt+Shift+Tab goes the other way**: from the start, to the least recently used.
- **The order only changes when Alt is let go**, so a round walks a fixed list. Where the round
  ended becomes the most recently used.
- **Windows never focused** (just opened ones with the keyboard elsewhere) follow the used ones in
  the bar's order, and a closed window drops out. Minimised windows are in the list and come back
  as before (ADR-0084); a fullscreen window with nothing else to go to still steps out to the
  desktop (ADR-0103).
- **Nothing new on screen**: there is still no switcher overlay, and the bar's window list (the
  tabs in the HUD) stays the one place windows are shown. The window being gone to is focused and
  raised as the round goes, and its tab lights up. The bar's order, and Super+1 to 9 by tab, are
  unchanged. Only which window Alt+Tab picks changed: the swapping behaviour.

## Consequences

- Alt+Tab matches other desktops, and the bar needn't be looked at to know where it goes.
- There is a hidden order, which ADR-0027 avoided: what Alt+Tab does depends on history, not on
  what's on screen. The bar's tab for the focused window shows where a held Alt has got to.
- Alt must be seen released to end a round; a round abandoned some other way (a click while Alt is
  held) ends the same way, with the round's last window as the most recently used.
