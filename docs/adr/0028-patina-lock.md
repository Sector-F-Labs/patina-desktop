# 0028. Patina's own lock screen, patina-lock, replaces swaylock as the default

Date: 2026-10-15

## Status

Accepted

## Context

ADR-0025 gave the compositor ext-session-lock and ran swaylock, a grey screen with a ring, which
must be installed separately and looks like nothing else on the desktop. It left room for a Patina
locker behind the same `[lock] command`, and the user asked for one.

A locker could be built into the compositor, which already hides the desktop while locked, but it
would then check passwords inside the display server, and ext-session-lock already gives a separate
client everything it needs. The shell pieces (bar, launcher, menus) are egui on layer-shell surfaces
through patina-shell's host (ADR-0011); a lock surface is drawn the same way, only with a different
role. Passwords are checked through PAM, as swaylock and login screens do; the pam crates need
bindgen or PAM's development files, which Ubuntu doesn't install, while the four calls needed are
short to declare.

## Decision

- **patina-lock** is a new workspace member: it locks the session through ext-session-lock, draws
  the time and date in the bar's skin on every output, and on the one with the keyboard, the user's
  name and a password box (dots only; Escape or Ctrl+U clear it, Enter checks it).
- patina-shell's host gains `Options::lock`: the host asks for the lock at once and puts a lock
  surface on every output, as it does layer surfaces; when the shell is finished, it unlocks before
  returning. Any other way out leaves the session locked.
- The password is checked on a thread through PAM, service `patina-lock`, declared in Rust and
  linked to the system's libpam (Linux-PAM or OpenPAM). `make install-session` installs
  `/etc/pam.d/patina-lock` (`auth include login`); without it PAM uses its `other` service.
- The password isn't kept in an egui TextEdit, whose undo history would copy it; it's read from the
  key events and zeroed once checked or cleared.
- `[lock] command` defaults to `patina-lock`; swaylock still works there.

## Consequences

- The lock screen matches the desktop and needs nothing installed beyond the workspace's binaries.
- patina-lock is now security-critical code of our own: a bug that ends it (a panic) leaves the
  session locked with nothing to type into, as a crashed swaylock does, until another locker is
  started, e.g. from a console.
- The password's bytes still pass through egui's input events and PAM's copies; zeroing is best
  effort, not a guarantee.
- No fingerprint, avatar or notifications on the lock screen yet; PAM modules that ask more than
  one question get the password for each.
