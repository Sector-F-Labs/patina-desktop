# 0120. The emoji picker is a popup that types the pick like dictation

Date: 2026-11-07

## Status

Accepted. The user asked for emoji insertion as macOS does it, with its shortcut and popup, and
for the same way of inserting as dictation. Builds on [ADR-0071](0071-text-is-typed-through-the-clipboard.md).

## Context

Nothing on the desktop inserts emoji. Typing text into another program's window is the
compositor's job (ADR-0071); dictation already does it through `Commit` on the session bus.

## Decision

- **`patina-emoji`, a program of its own** in the workspace: a layer-shell overlay on patina-shell,
  centred, with a search box, categories and a grid, in the skin. Installed by `make install`
  with a man page.
- **Opened by `emoji` in `[keys]`**, by default Ctrl+Super+Space (macOS's Ctrl+Cmd+Space) and
  Super+period. The compositor starts it as a client, like the launcher.
- **The pick is typed as dictation's words are**: the picker closes, waits 120 ms for the focus to
  come back, and calls `Commit`. No new compositor code for typing.
- **Emoji from the `emojis` crate** (names, shortcodes, groups), **drawn with Patina's
  `emoji` feature** (Patina's ADR-0031): Noto Color Emoji, the pack Teleprinter solved rendering
  with (its ADR-0013), painted as images because egui has no colour glyphs, so an emoji looks the
  same in every app. About 11 MB of binary. An emoji the font lacks (the `emojis` crate is newer) is
  left out of the grid. (Twemoji was the first try, and dropped for this.)
- **Recent emoji** are kept in `$XDG_DATA_HOME/patina-emoji/recent`.

## Consequences

- Skin-tone variants are left out of the grid for now; the default of each is typed.
- Wayland windows only, as dictation: X11 windows get nothing.
- Clipboard managers see the emoji go past, as with dictation.
