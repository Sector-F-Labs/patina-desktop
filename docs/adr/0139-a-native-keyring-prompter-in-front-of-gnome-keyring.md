# 0139. A native prompter asks for the keyring's password, in front of gnome-keyring

Date: 2026-11-13

## Status

Accepted

## Context

Apps keep passwords and tokens in the keyring through the Secret Service
(`org.freedesktop.secrets`). On Ubuntu `gnome-keyring-daemon` provides it, started by a systemd user
unit, and GDM's PAM stack (`pam_gnome_keyring`) unlocks the `login` keyring with the password typed
at login, so a normal login never asks twice. It asks when login can't unlock the keyring:
autologin or a fingerprint (no password to give PAM), a keyring whose password isn't the login one,
or a new keyring being created. The daemon doesn't draw that dialog. It calls a prompter on the
session bus (`org.gnome.keyring.SystemPrompter` and `PrivatePrompter`), which is GNOME's
`gcr-prompter`, a GTK program that looks out of place on Patina.

Three things were weighed:

- **Our own keyring daemon**: the security-critical part (the file format, locking, the Secret
  Service and portal protocols), with nothing to gain, since apps only see the D-Bus API. Not
  taken. `oo7-daemon` (Rust, from the GNOME and Flathub people) is the candidate if the GNOME
  dependency is ever to go.
- **Leaving `gcr-prompter`**: works, and costs nothing, but is the one GNOME-looking window a Patina
  user would see.
- **Our own prompter**: the dialog is the only part a user sees, and it is the same kind of piece as
  the polkit agent (ADR-0128).

The prompter protocol, `org.gnome.keyring.internal.Prompter`, is internal to gcr and not
documented ("it can change between releases"). It was read from gcr's source and checked against the
running daemon. Passwords never cross the bus in the clear: gcr's *secret exchange* (`[sx-aes-1]`)
does a Diffie-Hellman agreement on the 1536-bit IKE group, derives a key with HKDF-SHA256 and
encrypts with AES-128-CBC.

## Decision

- **A new crate, `patina-keyring`**, takes over both prompter names on the session bus and speaks
  gcr's protocol (`BeginPrompting`, `PerformPrompt`, `StopPrompting`, and `PromptReady` and
  `PromptDone` back on the client's callback). The compositor keeps it running, as the polkit agent
  and on the hardware only: nested, the parent desktop's prompter has the names.
- **The dialog is the same program run as `--ask`**, a patina-shell overlay in the skin, as in
  ADR-0128: a password, a new password with a repeat, or a plain question, with the daemon's tick box
  ("unlock automatically whenever I'm logged in") where it asks for one. One dialog at a time.
- **The secret exchange is written in Rust** (`exchange.rs`), from the RustCrypto crates and
  `num-bigint`, and tested against libgcr's own implementation (`tests/gcr-interop.py`).
- **gnome-keyring stays the daemon**, and logging in still unlocks it through PAM. This only
  replaces the window for the cases where that can't.
- The dialog reports a password strength with a new password (never 0 for one that was typed):
  gnome-keyring asks "Store passwords unencrypted?" when it reads 0.

## Consequences

- Where login can't unlock the keyring, the user sees a Patina dialog, not GTK, and nothing else
  changes: apps, the daemon and the keyring file are GNOME's.
- We depend on an internal protocol that GNOME may change. The two tests that run against the
  real libgcr and the real daemon (`tests/gcr-interop.py`, `tests/daemon-e2e.py`) are how that
  would show; neither runs in `make check`, as they need the GNOME libraries and a daemon.
- The password strength is a rough count (length and kinds of characters), not a real estimate,
  and the dialog doesn't show it. The daemon uses it only to skip the empty-password question.
- A keyring manager (listing and deleting entries, changing the keyring's password) isn't part of
  this. It would be a Settings page, on the same Secret Service.
- Not looked at yet: the daemon's prompts for changing a keyring's password, and what the dialog
  should do for the daemon's other questions beyond unlocking and creating.
