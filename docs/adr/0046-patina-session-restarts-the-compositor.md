# 0046. patina-session runs the compositor, and starts it again when it restarts or crashes

Date: 2026-10-17

## Status

Accepted

## Context

ADR-0045 made the desktop's pieces restartable from inside the session, but not the compositor.
GDM ends the session when the process its entry's `Exec=` names exits, and that was
`patina-compositor --session`. So trying a new compositor, or a compositor crash, meant the login
screen and logging in again.

Ways of restarting the compositor that were considered:

- **Re-executing itself in place** (i3's restart). Wayland clients' connections and their protocol
  state are in the compositor's memory, so they'd be lost anyway, and it doesn't help when the
  compositor crashes.
- **Keeping clients across a restart**, as KWin's wrapper does: it holds the listening socket and
  Qt 6 apps reconnect to the new compositor. That only works for toolkits that support
  reconnecting (GTK doesn't), and it's a large piece of work. Not now.
- **A small supervisor above the compositor**, as GDM's `Exec=`: it runs the compositor, starts it
  again when asked or after a crash, and ends the session on a clean exit. Windows close, but the
  login stays. This was chosen.

On the hardware, libseat and logind hand the GPU and input devices to the new compositor in the same
login session, so nothing else is needed for it to take the screen back.

A restart loses the lock screen's state too. A supervisor that restarts a crashed compositor would
open an unlocked desktop, so crashing the compositor would be a way past the lock screen.

## Decision

- **`patina-session`** (`session/`), a crate with no dependencies on the rest of the desktop, is
  what the login screen runs: `Exec=patina-session --session`. It runs `patina-compositor` (beside
  itself, else from `PATH`) with the arguments it was given, and:
  - a clean exit (Ctrl+Alt+Backspace) ends the session;
  - exit status **75** (EX_TEMPFAIL) is a restart: it starts the compositor again at once;
  - anything else is a crash: it starts it again a second later, unless the compositor has crashed
    three times within a minute, when it ends the session, back to the login screen, rather than
    loop;
  - SIGTERM, SIGHUP and SIGINT (the login manager ending the session, or Ctrl+C) are passed on to
    the compositor, and the session ends when it does.
- **The compositor knows it's supervised** by `PATINA_SESSION`, which patina-session sets to its
  own process ID. The compositor's clients don't inherit it.
- **Restarting is asked for** with **Ctrl+Alt+Shift+Backspace**, or with Restart Desktop on
  Settings' Processes page, which calls `RestartDesktop()` on the compositor's
  `com.sectorflabs.Patina.Processes` (ADR-0045), with a `Supervised` property saying whether it can.
  The compositor stops its event loop and exits with 75. It refuses without patina-session, where a
  restart would end the session, and while the screen is locked.
- **The lock survives a restart.** While the screen is locked, the compositor keeps a marker,
  `$XDG_RUNTIME_DIR/patina-session-<ID>.locked`. A compositor started under patina-session that
  finds the marker starts locked, drawing black until the locker it starts draws, before its first
  frame. patina-session removes the marker when the session ends.
- **The test VM** runs `patina-session` too (under `dbus-run-session`), so restarting is tried
  where the desktop is worked on.

## Consequences

- Trying a new compositor in a session: `make install`, then Ctrl+Alt+Shift+Backspace. The login
  stays; every window closes.
- A crash costs the open windows, not the login. Three in a minute still end at the login screen,
  with the reason in patina-session's output, in the journal.
- `pkill patina-compositor` now restarts the desktop rather than ending the session; ending it from
  a console is `pkill patina-session`.
- Only patina-session's own process ID ties the marker to the session. If it were killed with
  SIGKILL, the marker would stay until logout clears `XDG_RUNTIME_DIR`, which does no harm, since a
  new session has a new ID.
- Restarting the compositor still closes every app. Keeping them would take reconnecting clients,
  as above.
