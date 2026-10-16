# patina-lock

The lock screen: the time and date in the bar's skin on every screen, and a password box that
unlocks once PAM accepts the password (ADR-0028).

## On the desktop

The compositor locks with Ctrl+Super+Q, Super+Escape, and before the machine sleeps. Every
output shows the time and date; the one with the keyboard also shows who's locked and a
password box. Typing shows only dots; Enter checks the password, and a wrong one is cleared with "Wrong
password" under the box.

## How it fits in

- **Started by** patina-compositor, as its `[lock] command` (`patina-lock` by default,
  ADR-0025): on the lock keys, and on logind's PrepareForSleep when `on_suspend` is set, so
  waking never shows the desktop.
- **Locks through ext-session-lock.** patina-shell's host, with `Options::lock`, asks the
  compositor for a lock surface on each output. From then on the compositor draws and sends
  input only to those; if the locker crashes, the session stays locked. Unlocking is the
  locker's to do, once the password is right.
- **Checks the password through PAM** (`pam.rs`), with the service `/etc/pam.d/patina-lock`,
  which `make install-session` installs (without it PAM falls back to `other`). The few PAM calls
  are declared by hand; Linux-PAM and FreeBSD's OpenPAM share them.
- **The password** is never kept in a text field, whose undo history would hold copies: keys are
  read straight from egui's input and the buffer is wiped once checked.
- **The skin** comes from `PATINA_SKIN` or the bar's config, like every piece.

## Code

`main.rs` (the lock and the PAM thread), `ui.rs` (the clock and password box), `pam.rs`.

## Running

It needs a compositor with ext-session-lock, such as patina-compositor:
`cargo run -p patina-lock` inside `make run` locks that nested session.
