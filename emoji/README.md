# patina-emoji

The emoji picker (ADR-0120), as Ctrl+Cmd+Space on a Mac. The compositor opens it on
Ctrl+Super+Space or Super+period (`emoji` in `[keys]`): a popup in the middle of the screen with a
search box, the categories along the top and the emoji in a grid. Enter or a click picks one, and it is
typed into the window that had the focus.

## How it fits in

- **Typing is dictation's** (ADR-0071): the picker closes, waits a moment for the focus to return, and
  calls `Commit(emoji)` on `com.sectorflabs.Patina.Input`; the compositor pastes it through the
  clipboard and gives the clipboard back. Wayland windows only, as dictation.
- **A layer-shell overlay** on patina-shell, like the launcher: keyboard exclusive, closes on
  Escape or when another window is clicked. Running it while open closes it, so the key toggles it.
- **Twemoji**, as images (`twemoji-assets`, graphics CC-BY 4.0 by Twitter and other contributors),
  because egui's fonts have no colour emoji. They are decoded as they scroll into view.

## Code

`main.rs`, the surface and the `Commit`; `ui.rs`, the search box, categories and grid; `data.rs`,
the emoji (the `emojis` crate), searching them by name and shortcode, and the recent ones kept in
`$XDG_DATA_HOME/patina-emoji/recent`.
