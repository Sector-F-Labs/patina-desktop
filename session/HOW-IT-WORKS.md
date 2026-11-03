# How a Patina session runs

Log in, and the desktop appears; press Ctrl+Alt+Shift+Backspace, and it comes back a moment
later without the login screen; have it crash, and the same. This page explains the small
program that makes that so, and what it does and doesn't hold on to. The README next to it
covers running and testing; the decisions are in ADR-0046 and ADR-0083.

## The idea in one paragraph

A login manager like GDM ends the session when the program it started exits. If that program were
the compositor, restarting the compositor, to try a new build or after a crash, would mean
the login screen and logging in again. So the login manager is given a thin supervisor,
`patina-session`, instead. It runs the compositor and starts it again when asked or after a
crash, and ends only when it should. Windows still close on a restart, since their connections
die with the compositor, but the login stays and nothing else needs to.

## The picture

<img src="images/architecture.svg" alt="The login manager runs patina-session. It reads the login profile once, then runs patina-compositor with PATINA_SESSION set to its own process ID. The compositor starts and restarts the wallpaper, bar, tiler and dictate programs. The login manager's signals are passed on to the compositor, and a marker file in the runtime directory remembers the lock across restarts.">

`patina-session` has no dependency on the rest of the desktop. It knows one program by name
(`patina-compositor`, found beside itself, else on `PATH`) and passes on whatever arguments it
was given, which is how `--session` reaches the compositor.

## What it does when the compositor exits

<img src="images/exits.svg" alt="A signal from the login manager, or exit status 0, ends the session. Status 75 starts the compositor again at once. Any other status is a crash: after a second it starts again, unless it is the third crash within a minute, which ends the session as a failure.">

The compositor's exit status is the whole protocol:

- **0** is Ctrl+Alt+Backspace, the way out of a session.
- **75** (`EX_TEMPFAIL`) means "start me again". The compositor exits with it on
  Ctrl+Alt+Shift+Backspace and on Restart Desktop in Settings' Processes page.
- **Anything else** is a crash. It starts again after a second, but three crashes within a minute
  end the session, as a failure, so a compositor that can't start leaves a login screen to
  go back to rather than a loop with no way out.
- **A signal from the login manager** (SIGTERM, SIGHUP) or Ctrl+C (SIGINT) is passed on to the
  compositor, and the session ends when it does. It is never started again after that.

## Why the compositor has to know

The compositor is told it is supervised with the environment variable `PATINA_SESSION`, set to
the supervisor's process ID. Its clients don't inherit it. It decides two things on that basis:

- **Whether a restart is allowed.** `RestartDesktop()` on the Processes interface exists, and
  `Supervised` says whether it will work. Without a supervisor, a restart would just end the
  session, so it refuses. It also refuses while the screen is locked.
- **Where its lock marker goes.** The ID in the file's name ties the marker to this session.

## The lock survives a restart

A supervisor that restarts a crashed compositor would otherwise be a way past the lock screen:
crash it, and the desktop comes up unlocked. So while the screen is locked, the compositor keeps
a marker in `$XDG_RUNTIME_DIR/patina-session-<id>.locked`. A compositor started under
`patina-session` that finds the marker starts locked, drawing black until the locker it starts
draws, before its first frame. `patina-session` removes the marker only when the session ends.

A restart in between keeps it; so does a crash. If `patina-session` is killed with SIGKILL the
marker stays until logout clears the runtime directory, which does no harm, since a new session
has a new ID.

## The environment, read once

The login manager runs `patina-session --session` straight from the session entry, with no
shell, so nothing reads `~/.profile`. That is where Ubuntu puts `~/.local/bin` on `PATH`, and
where people set `EDITOR` and `~/.cargo/bin`. Before it starts the compositor, `patina-session`
runs `/bin/sh -l -c 'exec env -0'` and hands what it prints (less the shell's own `SHLVL`, `_`,
`PWD` and `OLDPWD`) to the compositor, which its clients inherit (ADR-0083).

A profile can't be allowed to stop a login. It gets no input, its output goes nowhere, and it
has five seconds. If it fails, takes longer, or starts something that keeps its output open, the
session logs why and goes on with the login screen's environment. It is read once per session,
not on each restart, so a change to `~/.profile` takes a new login.

## What it doesn't do

- It doesn't keep windows across a restart. That would take reconnecting clients: KWin does it
  with a socket it holds, and only for toolkits that support it. Not now.
- It doesn't watch the compositor's own clients. The compositor starts the wallpaper, the bar,
  the tiler and dictation, and starts them again itself (ADR-0045); the supervisor sees only
  one process.
- It doesn't start the lock screen, apps or anything else.

## The files

```
 patina-desktop/
 ├── session/src/main.rs        the supervisor: login profile, run, restart, signals, lock marker
 ├── compositor/src/session.rs  PATINA_SESSION, the exit status 75, the lock marker
 ├── compositor/src/session/dbus.rs   locking and logging out for the bar's power menu
 ├── compositor/src/processes/  RestartDesktop() and Supervised on the Processes interface
 └── packaging/linux/patina.desktop   Exec=patina-session --session
```

The diagrams in `images/` are hand-drawn SVG, and follow the viewer's light or dark theme.
Its tests supervise stand-in compositors (shell scripts) through each case above. The test VM
runs `patina-session` too, so restarting is tried where the desktop is worked on.
