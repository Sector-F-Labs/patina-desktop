# 0083. The session takes the environment a login shell sets up

Date: 2026-10-23

## Status

Accepted. On nifl, the clock's menu said Calendar wasn't installed although `make install` had
put it in `~/.local/bin`. The user chose reading the login profile over adding that one folder.

## Context

The login screen runs `patina-session --session` straight from the session entry, without a
shell. So nothing reads `~/.profile`, where Ubuntu puts `~/.local/bin` on `PATH` (and where people
set `EDITOR`, `~/.cargo/bin` and the rest). Every app on the desktop got the login screen's bare
`PATH`, and anything installed per user (Calendar's `make install`, pipx, `cargo install`) could
only be started by its full path. GNOME on the same machine has them, because GDM's session reads
the profile; X display managers do the same in their Xsession script.

Considered:

- **Add `~/.local/bin` to the apps' `PATH`** in the compositor, beside its own folder. Small, but it
  fixes one folder and none of the rest a profile sets.
- **Read the login profile**, as other desktops' sessions do.
- **The user's own shell** (`$SHELL -l`) rather than `sh`. zsh, nifl's shell, reads `~/.zprofile`
  as a login shell, not `~/.profile`, so it would miss what Ubuntu sets up.

## Decision

patina-session runs `/bin/sh -l -c 'exec env -0'` before it starts the compositor, and gives the
compositor the environment it prints (less the shell's own `SHLVL`, `_`, `PWD` and `OLDPWD`); the
compositor's clients inherit it. It's read once per session, not on each restart.

A profile mustn't be able to stop a login. It gets no input and its output goes nowhere. If it
fails, takes over five seconds, or starts something that keeps its output open, patina-session
logs why and goes on with the environment the login screen gave.

## Consequences

- Per-user installs work by name on the desktop, as on GNOME.
- A profile that prints or prompts does nothing visible. One that's slow delays the login by up to
  five seconds, then is ignored.
- A change to `~/.profile` takes a new login, as elsewhere; a desktop restart reuses what was read.
