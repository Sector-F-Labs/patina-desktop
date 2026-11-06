# 0021. A tray with a volume menu that doubles as the volume keys' display; Super opens the launcher

Date: 2026-10-14

## Status

Accepted

## Context

The volume keys worked (ADR-0020) but showed nothing. The user asked for a system tray with a volume
button whose menu has a slider, and for that same menu to fade in briefly when the volume keys are
used. They also asked for the Super key on its own to open the launcher.

## Decision

- **patina-volume**, a new shell piece on patina-shell: a menu above the bar's right end with a mute
  button, a slider and the level, read and set through PipeWire's `wpctl` (a module shared with the
  bar in `patina_shell::audio`). As a **menu** (run with no arguments, by the bar's speaker) it takes
  the keyboard and closes on Escape, on losing the keyboard (a click elsewhere) or on being run again.
  As a **flash** (`--flash`, run by the compositor after a volume key) it leaves the keyboard alone,
  fades in, holds 1.5 s and fades out; another key press holds it again, and the speaker turns it
  into the menu. A second run talks to the first over a socket in `XDG_RUNTIME_DIR`, as the launcher
  does.
- **The bar's `tray` module**, on by default at the right: a speaker drawn from shapes in the skin's
  colours (`patina_shell::icons`), showing the level and mute, read once a second.
- **The Super key**, tapped on its own, runs the compositor config's `launcher` (patina-launcher);
  any other key or a click while it's held (Super+Space, Super+drag) cancels it.
- Pieces started by the compositor rather than the bar read the bar's skin and edge from its config
  file (`patina_shell::hud`), so they match it.
- patina-shell's host can change a surface's keyboard interactivity at runtime (`set_keyboard`), for
  the flash becoming the menu.

## Consequences

- The level shows wherever it changes: the speaker's waves, and the flash after a key.
- Brightness has no display yet; the same flash could carry it.
- The tray holds only the speaker. Other apps' tray icons (StatusNotifierItem over D-Bus) are a
  separate, larger piece.
- Volume control assumes PipeWire; other sound systems need another `audio` backend.
