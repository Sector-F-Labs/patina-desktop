# 0005. Keep the server login in a file only the user can read

Date: 2026-10-11

## Status

Proposed. This is my choice.

## Context

Calendar needs the CalDAV server's address, username and password. `plan` keeps its own password
in plain text in `~/.config/plan/config.toml`. GNOME's keyring (Secret Service) is safer, but
would add a D-Bus and async dependency. Copying `plan`'s password into the app automatically was
refused as credential handling. The user enters it themselves.

## Decision

- **Where:** the account lives in `~/.config/calendar/account.toml`, apart from the other
  settings, written with mode 600.
- **How it's entered:** File → Account… edits it. It opens by itself on first run, when there's
  no account. Connecting replaces the cached data and refetches.

## Consequences

- One plain-text secret on disk, protected by file permissions, like `plan`'s own.
- **Revisit** moving it into the Secret Service if the app is used on shared machines, or once
  another Patina app needs credentials too. That would be a Patina component.
