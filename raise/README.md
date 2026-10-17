# patina-raise

Focus an app's window, or start the app if none is open (ADR-0058): what a key binding to an app
wants. `patina-raise firefox` on Alt+B (the compositor's default) focuses Firefox if it's open,
steps to its next window if it already has the focus, and starts it if not. `-i` ignores case
and `-p` matches part of a name, so `patina-raise -ip telegram` finds Telegram Desktop; names
come from the windows' app ids and their `.desktop` entries (ADR-0060).

## How it fits in

- **Run by** a key binding: the compositor's `[bindings]`, or wbindkeys, or any other.
- **Sees windows** through wlr-foreign-toplevel-management, as the bar does, so it works under
  other wlroots compositors too; under one without it (GNOME) it only starts the app.
- **Starts the app** through the compositor's `Spawn` (ADR-0045), so it's among the desktop's
  processes, or itself where nothing serves that.
- **Starts the app** by its `.desktop` entry's Exec when no command is given, through
  `patina_system::apps`, which the bar's tab icons use too.
- `--list` prints the open windows' app ids and names, to find the name to give it.

## Code

`choose.rs`, the matching (`Matching`) and which window to focus or whether to start, with its
tests; `main.rs`, the arguments, what starts the app, and the Wayland client.

## Running

```sh
cargo run -p patina-raise -- --list
cargo run -p patina-raise -- firefox
```
