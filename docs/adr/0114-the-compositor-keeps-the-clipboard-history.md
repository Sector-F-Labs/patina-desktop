# 0114. The compositor keeps the clipboard history, and serves it on the session bus

Date: 2026-11-07

## Status

Proposed. The user asked whether a clipboard manager is possible on Wayland, and wanted a tray
icon that opens a small applet to pick or delete items, with images shown. They agreed the
compositor should own the history, and that the interface would be D-Bus.

## Context

On Wayland a client can read the clipboard only while it has focus, and a selection is a pointer
to its source app: when that app exits, the copied data is gone. A clipboard manager must watch
every selection and keep a copy of it.

Options considered:

- **A daemon on `ext-data-control-v1` or `wlr-data-control`**, as `cliphist` and
  `wl-clip-persist` do. It needs the compositor to expose that protocol, and a background
  process that holds the data.
- **The compositor itself.** Every copy already goes through it (Smithay's
  `SelectionHandler::new_selection`), and it can set the selection itself. It needs no extra
  protocol and no extra process. The cost is more code in the compositor.

The compositor already serves session-bus interfaces with `zbus` for the pieces to use:
`com.sectorflabs.Patina.Session` and `com.sectorflabs.Patina.Keyboard` (ADR-0068). The HUD's
tray icons already open pieces of their own, such as volume and Wi-Fi (ADR-0021, ADR-0022,
ADR-0048). `patina-menu` takes text labels only (ADR-0056), so it can't show images.

## Decision

- **The compositor keeps the history** (`compositor/src/clipboard/`).
  - `new_selection` reads the offered data through the selection's fd. Text first, then images
    in the types that come as PNG or similar.
  - When the source app exits, the compositor sets its latest entry as the selection, so a copy
    outlives the app.
  - Selections that carry `x-kde-passwordManagerHint` are skipped.
  - Caps: a few MB per entry and 50 entries, the oldest images dropped first.
  - Images are kept as files, with a thumbnail made when the entry is stored, in
    `$XDG_RUNTIME_DIR/patina/clipboard/`.
  - History can be turned off or paused, and entries expire, so something that slipped past the
    password filter doesn't stay.
- **`com.sectorflabs.Patina.Clipboard` on the session bus**, built like the Keyboard interface.
  - zbus runs on threads of its own. Requests reach the event loop over a calloop channel,
    because only the event loop touches Smithay's state.
  - Methods: `List() -> a(tsssu)` (id, kind, preview, thumbnail path, time), `Pick(id)`,
    `Delete(id)`, `Clear()`.
  - Signal: `Changed`, without a payload. Listeners call `List` again.
  - Properties: `Enabled` and `Paused`, readable and writable.
  - `List` returns truncated previews only. The full content is reachable only by `Pick`, which
    sets the selection.
  - Images never cross the bus. `List` gives the thumbnail's path, and the applet loads the
    file.
- **The applet is a piece of its own** (`patina-clipboard`), opened from a HUD tray icon, like
  volume and Wi-Fi. A row per entry: a text preview or a thumbnail. Clicking copies it and
  closes the applet, × deletes it, and Clear all empties the list.
- **No synthetic paste.** The user presses Ctrl+V.
- **Wayland only.** No X11 clipboard bridge.
- **Text first.** Images come in a second step, with the thumbnails and the files.

## Consequences

- Copies survive their app closing, which Wayland doesn't give by itself. The tray icon and applet
  update live from `Changed`.
- The compositor grows: selection handling, a store, thumbnails. It should live in its own module
  so the rest stays clear of it.
- **The session bus is open to every program the user runs.** Any of them can call `List` and
  read a preview of every entry, which is weaker than Wayland's model, where only the focused
  window can read the clipboard. They can also call `Pick` to set the selection, but not read
  it. Truncated previews reduce this and don't remove it. We accept it, as other Linux clipboard
  managers do, and it is the reason to keep `Paused` and expiry.
- Anything that slips past the password filter is stored, in memory and on disk, until it expires.
- Rich types (HTML, files) aren't kept at first, so pasting rich text from history gives plain
  text.
- Revisit if the bus exposure proves too much. A socket the compositor hands only to chosen
  pieces would be the next step, since the same user runs every program and a socket alone
  wouldn't help.
