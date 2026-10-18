# 0003. Keep the Bridge login in a private file

Date: 2026-10-18

## Status

Proposed

## Context

Mail needs Bridge's username and generated password every time it connects. The options were the
desktop keyring (Secret Service over D-Bus) or a file in the app's config folder. Calendar keeps its
CalDAV login in a separate `account.toml` readable only by the user (its ADR-0005), and Bridge's
password only works against Bridge on this machine, not against the Proton account itself.

## Decision

We will keep the login in `~/.config/mail/account.toml`, created with mode 0600 and separate from
`config.toml`, entered through File → Account….

## Consequences

- Same pattern as Calendar; no keyring dependency or D-Bus prompts.
- Anyone who can read your home folder as you can read the Bridge password. That password only
  opens Bridge on this machine, which limits the damage, but a keyring would be stronger. Revisit
  if Mail ever talks to something other than a local Bridge.
