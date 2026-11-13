# patina-keyring

The keyring's password prompt (ADR-0139): the dialog that asks for a keyring's password when
logging in couldn't unlock it.

## On the desktop

gnome-keyring is the keyring and is unlocked at login with your login password (GDM's PAM stack),
so a normal login never shows this. It opens when the daemon needs a password it didn't get:
after an automatic or fingerprint login, when the keyring has a different password than your
login, or when an app creates a new keyring. Enter goes on, Escape or Cancel refuses; a wrong
password is asked for again.

## How it fits in

- **Started by** the compositor, kept running like the polkit agent, on the hardware only. Nested,
  the parent desktop's prompter has the names.
- **Takes** `org.gnome.keyring.SystemPrompter` and `PrivatePrompter` on the session bus, where
  `gcr-prompter` (GNOME's) would answer, and speaks gcr's internal prompter protocol: the daemon
  calls `BeginPrompting`, `PerformPrompt` and `StopPrompting`; we call `PromptReady` and
  `PromptDone` on its callback object.
- **Shows** each prompt by running itself as `patina-keyring --ask ...`, a patina-shell overlay (as
  patina-polkit does), one at a time. It answers on stdout.
- **Passes the password** under gcr's secret exchange (`[sx-aes-1]`): Diffie-Hellman on the
  1536-bit IKE group, HKDF-SHA256, AES-128-CBC. The password is never kept or logged.

## Code

`main.rs` (the two modes), `agent.rs` (the bus and the dialog's child process), `exchange.rs`
(the secret exchange), `dialog.rs` (the overlay).

## Testing

- `cargo test -p patina-keyring` runs the unit tests.
- `python3 -I keyring/tests/gcr-interop.py` checks `exchange.rs` against libgcr's own secret exchange.
- `python3 -I keyring/tests/daemon-e2e.py` (after `cargo build -p patina-keyring`) runs the whole
  thing against the real gnome-keyring-daemon on a bus and in a data directory of their own, with a
  stub where the dialog would be: creating a keyring, a wrong password, unlocking.
- `PATINA_KEYRING_PREVIEW=/tmp/p cargo test -p patina-keyring preview -- --ignored` paints the
  dialog in every built-in skin as PNGs, with no desktop needed.
