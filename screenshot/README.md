# patina-screenshot

What the Print key (or Super+Shift+5) opens (ADR-0091): a pill at the bottom of the screen, like
dictation's overlay, offering the whole screen, the window that was focused, or a selection.

## On the desktop

- **Screen, Window, Selection**: a click, Enter or Space on one chooses it; the arrows and Tab
  move between them, and 1 to 3 choose one outright. Escape, the close button, or the keyboard
  going elsewhere closes it without taking anything.
- **A selection** is then dragged out with the pointer over the dimmed screen; letting go takes
  it, and Escape, another button or a click without a drag takes nothing.
- Screenshots land in `~/Pictures/Screenshots`, named as macOS names them (ADR-0034).

## How it fits in

- **Started by** the compositor on its `screenshot_menu` key, with `--no-window` when no window
  is focused. It prints `screen`, `window` or `selection`, or nothing, and exits.
- **Takes nothing itself.** The compositor reads the choice once it has exited and draws the
  picture (`compositor/src/screenshot/menu.rs`), or runs the selection
  (`compositor/src/screenshot/select.rs`).
- `PATINA_SCREENSHOT_PREVIEW=dir cargo test -p patina-screenshot preview -- --ignored` draws it
  in each skin as PNGs.
