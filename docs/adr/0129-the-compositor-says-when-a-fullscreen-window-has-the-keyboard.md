# 0129. The compositor says when a fullscreen window has the keyboard, and dictation puts its model away then

Date: 2026-11-09

## Status

Accepted. Builds on [ADR-0041](0041-fullscreen.md), [ADR-0102](0102-print-takes-the-screen-when-something-is-fullscreen.md)
and [ADR-0072](0072-dictation-is-a-held-key-and-a-daemon.md).

## Context

The compositor already knows when a fullscreen window, a game or a video, has the keyboard
(`State::fullscreen_on_top`), and acts on it: the desktop's keys stay out of the game's way
(ADR-0102), remaps are off. Nothing outside the compositor could know.

Meanwhile patina-dictate's daemon loads its Whisper model at the first press and keeps it for the
whole session: 0.5 to 1.6 GB of memory, on the graphics card where there is one. A game wants that
memory, and nobody dictates into a game, or while a video plays, so it's held for nothing. Nothing
ever freed it, either, however long it went unused, and the first press of a session waited for
the load.

## Decision

- **The compositor announces it**: `com.sectorflabs.Patina.Input` gets the signal
  `FullscreenChanged(b)` and the read-only property `Fullscreen`, true while a fullscreen window
  has the keyboard and the screen isn't locked (the test the keys already use). It's checked each
  time round the event loop and signalled only on a change; the property is for a listener that
  starts later. It's a fact about the desktop, free to anything on the bus, not tied to dictation.
- **The daemon puts the model away**: `unload_fullscreen_secs` after a fullscreen window gains the
  keyboard (default 10), and `unload_idle_mins` after the model was last used (default 10), either
  0 for never. The next press loads it again, which takes a second or two.
- **`preload`** (default off) loads the model when the daemon starts, so the first press doesn't wait,
  and again when a fullscreen window is left, so coming back from a game is ready.
- **Settings' Dictation page has a Memory section** with all three, written to patina-dictate's
  config, which the daemon reads as it goes.
- **The daemon's model thread** waits for work with a deadline instead of forever: a recording, a
  fullscreen change, or the time to unload.

## Consequences

- Playing a game or watching a video gives the memory back after ten seconds, and a desktop left
  alone for ten minutes gives it back too.
- The price is a load at the first press afterwards (about 0.2 to 1.3 s measured, more for the full
  large models); `preload` or a 0 buys it back.
- A fullscreen window the user dictates into, a browser in fullscreen, would unload the model too
  and load it at the next press. The dictate key does nothing in fullscreen (ADR-0102), so this
  costs nothing there.
- Other things can use the signal: pausing background work, quieting notifications.
- An older compositor without the property: the daemon takes it as "not fullscreen", and only the
  idle unload works.
