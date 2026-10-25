# patina-screenshot

What the Print key (or Super+Shift+5) opens (ADR-0091, ADR-0092): a pill at the bottom of the
screen, like dictation's overlay, offering the whole screen, the window that was focused, or a
selection; then recording the screen or a selection. Also the pill shown while recording, and
the chooser when an app asks to share the screen.

## On the desktop

- **Screen, Window, Selection, Record Screen, Record Selection**: a click, Enter or Space on
  one chooses it; the arrows and Tab move between them, and 1 to 5 choose one outright. Escape,
  the close button, or the keyboard going elsewhere closes it without taking anything.
- **A selection** is then dragged out with the pointer over the dimmed screen; letting go takes
  it (or starts recording it), and Escape, another button or a click without a drag takes
  nothing.
- **While recording**, a pill at the bottom shows a red dot, the time and Stop; Stop or the
  Print key again ends it.
- **Sharing the screen** (OBS, a browser): a panel in the middle of the screen lists the screen
  and the windows; the app sees only the one chosen, and nothing on Escape.
- Screenshots land in `~/Pictures/Screenshots` (ADR-0034), recordings in
  `~/Videos/Screen Recordings`, named as macOS names them.

## How it fits in

- **Started by** the compositor on its `screenshot_menu` key, with `--no-window` when no window
  is focused. It prints `screen`, `window`, `selection`, `record-screen` or `record-selection`,
  or nothing, and exits.
- **Takes and records nothing itself.** The compositor reads the choice once it has exited and
  draws the picture (`compositor/src/screenshot/menu.rs`), runs the selection
  (`compositor/src/screenshot/select.rs`) or records (`compositor/src/capture/record.rs`, with
  ffmpeg).
- **`--recording`** (`indicator.rs`) is started with a recording; the compositor closes its
  stdin when the recording ends, and it prints `stop` if Stop is pressed.
- **`--share`** (`share.rs`) is xdg-desktop-portal-wlr's chooser (`chooser_type=dmenu`, set in
  `/etc/xdg/xdg-desktop-portal-wlr/Patina` by `make install`): it reads the portal's
  `Monitor: …` and `Window: …` lines on stdin and prints the one chosen.
- `PATINA_SCREENSHOT_PREVIEW=dir cargo test -p patina-screenshot preview -- --ignored` draws the
  pill in each skin, the recording's pill and the chooser as PNGs.
