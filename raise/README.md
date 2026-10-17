# patina-raise

Focus an app's window, or start the app if none is open (ADR-0058): what a key binding to an app
wants. `patina-raise firefox` on Alt+B (the compositor's default) focuses Firefox if it's open,
steps to its next window if it already has the focus, and starts it if not.

## How it fits in

- **Run by** a key binding: the compositor's `[bindings]`, or wbindkeys, or any other.
- **Sees windows** through wlr-foreign-toplevel-management, as the bar does, so it works under
  other wlroots compositors too; under one without it (GNOME) it only starts the app.
- **Starts the app** through the compositor's `Spawn` (ADR-0045), so it's among the desktop's
  processes, or itself where nothing serves that.
- `--list` prints the open windows' app ids, to find the name to give it.

## Code

`choose.rs`, which window to focus or whether to start, with its tests; `main.rs`, the
arguments and the Wayland client.

## Running

```sh
cargo run -p patina-raise -- --list
cargo run -p patina-raise -- firefox
```
