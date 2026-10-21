# patina-session

The Patina desktop's login session (ADR-0046): what the login screen runs. It runs
patina-compositor and starts it again when the compositor asks to restart or crashes, so neither
means logging in again.

## How it fits in

- **Started by** the login screen, from `packaging/linux/patina.desktop`
  (`Exec=patina-session --session`), which `make install` installs for every user (ADR-0078). Its arguments go to
  the compositor.
- **Runs patina-compositor**, found beside it (as `make install` and the build put them) or else
  on `PATH`, with `PATINA_SESSION` set to its own process ID, so the compositor knows a restart
  won't end the session.
- **What the compositor's exit means:** 0 (Ctrl+Alt+Backspace) ends the session; 75 restarts it
  (Ctrl+Alt+Shift+Backspace, or Restart Desktop in Settings' Processes page); anything else is a
  crash, after which it's started again a second later. Three crashes within a minute end the
  session.
- **The login manager's SIGTERM or SIGHUP** is passed on to the compositor, and the session ends.
- **The lock screen survives a restart:** the compositor keeps a marker in `XDG_RUNTIME_DIR` while
  locked and starts locked when it finds one; patina-session removes it when the session ends.

## Code

`main.rs`. Its tests supervise stand-in compositors (shell scripts) through each case.

## Running

```sh
cargo run -p patina-session            # nested in the current desktop, like make run
```
