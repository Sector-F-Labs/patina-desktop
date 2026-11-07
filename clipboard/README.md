# patina-clipboard

The clipboard menu in the bar's tray (ADR-0114): what was copied lately, to copy again, delete or
clear.

## On the desktop

From the bar's clipboard icon, at the bar's right end: a header with Pause and Clear all, then the
copies, newest first, each with how long ago. A click copies it again, so Ctrl+V pastes it, and
closes the menu; the cross deletes it. An image shows as a thumbnail. Escape, a click elsewhere or
the icon again closes it.

## How it fits in

- **The compositor keeps the history** (`compositor/src/clipboard.rs`), so it's there while this is
  closed, and a copy outlives the app it came from. This only shows it and sends it requests.
- **Talks to** `com.sectorflabs.Patina.Clipboard` on the session bus through patina-system's
  `clipboard.rs`: `List`, `Pick`, `Delete`, `Clear`, the `Paused` property, and the `Changed`
  signal, which the list follows. Nothing serves it under another desktop, and the bar then shows
  no icon.
- **Thumbnails** are small PNG files the compositor writes under
  `$XDG_RUNTIME_DIR/patina/clipboard/`; the menu reads them, so no image crosses the bus.
- **Started by** the bar's clipboard icon (its `clipboard` command), with the bar's skin and
  edge. A second run closes the open one.

## Code

`main.rs` (one copy, the threads, the surface), `ui.rs` (the list, the rows, the thumbnails).

## Running

```sh
cargo run -p patina-clipboard
```

Needs the compositor's history on the session bus, as under `make run`.
