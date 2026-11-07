# 0116. Screenshots always go to the clipboard, and `[clipboard]` in the compositor config with a Settings page

Date: 2026-11-07

## Status

Accepted. The user wanted Super+Shift+3 and Super+Shift+4 screenshots to reach the clipboard, and
a setting so people can turn the clipboard history off. They chose that screenshots always go
there, and still do with the history off.

## Context

Screenshots are saved as PNG files in `~/Pictures/Screenshots` (ADR-0034, ADR-0091). Nothing
reaches the clipboard, so pasting one into a chat or a browser means finding the file first.

On a Mac, Cmd+Shift+3 and Cmd+Shift+4 save a file, and adding Ctrl copies to the clipboard
instead. Here Super is Cmd (ADR-0067), so that would be Super+Ctrl+Shift+3 and 4, next to
Ctrl+Super+Q, which locks. It would also be a second set of keys to learn.

The clipboard history (ADR-0114) has `Enabled` and `Paused` as D-Bus properties. They reset when
the compositor restarts, and the tray menu's Pause is only for the session. Nothing in Settings
touches the clipboard. The compositor config is reloaded within a second of a save, and Settings
already writes it for gaps and shadows (ADR-0051, ADR-0112).

## Decision

Built: the compositor side (the `[clipboard]` config, the history cleared when turned off,
screenshots on the clipboard, the HUD icon hidden while it's off) and the Settings page. Not yet:
expiry.

- **Every screenshot goes to the clipboard as well as to its file.** It is the whole desktop,
  the window or the selection, from Super+Shift+3, 4 or 5 and the Print key. There is no
  Ctrl variant. The picture is put on the selection as `image/png`, as a pick of a history entry
  is, so Ctrl+V pastes it.
- **With the history off it is still copied**, but not kept: the screenshot setting doesn't
  depend on the history one. With the history on, the screenshot is also an entry, with a
  thumbnail like any image.
- **Recordings aren't copied.** A video is too big, and Ctrl+V of one rarely means anything.
- **A `[clipboard]` section in the compositor's config**, hot-reloaded like the others, and every
  key optional:
  - `enabled` (default true): keep a history.
  - `screenshots` (default true): put screenshots on the clipboard.
  - `max_entries` (default 50, as now), and later `expire`.
- **Turning the history off clears it**, entries and thumbnails, instead of leaving them. Someone
  who turns it off is likely doing it for privacy. The compositor then says nothing is kept, and
  the bar's icon goes.
- **The bus properties stay runtime switches.** `Paused` from the tray menu lasts for the
  session. A config change sets `Enabled` again when it is read.
- **A Clipboard page in Settings**, written to that config: Keep a history, Copy screenshots to
  the clipboard, and a Clear history button, which calls `Clear`. Entry count and expiry go on
  the same page when they exist.

## Consequences

- Screenshots can be pasted straight away, which most people expect from the key.
- Every screenshot is in the history while it is on, and a full-screen one is several MB. The
  caps (16 MB an entry, 128 MB in all, images dropped first) bound it. Anyone who doesn't want
  screenshots in the history turns the history off and keeps the copying.
- A screenshot can show private things, and now sits on the clipboard where any program the
  user runs can ask for it, as with any copy. The bus doesn't give out content (ADR-0114), but
  the Wayland clipboard does, to the focused window.
- Replacing what was on the clipboard is a cost: taking a screenshot overwrites a copied text.
  With the history on it's still there in the menu; with it off it isn't. We accept that, since
  it's the same as on the other desktops.
- The compositor's screenshot code grows a small step after saving, and the clipboard module
  gets an entry point for pictures that aren't read from an app.
- No Ctrl variant to save without copying. If that is missed, `screenshots = false` is the
  answer, not new keys.
