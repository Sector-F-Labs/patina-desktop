# 0002. Keep the Home Assistant token in a private file

Date: 2026-10-28

## Status

Proposed

## Context

Home needs the Home Assistant address and a long-lived access token, which gives full control of
the house. Calendar solved the same problem for its CalDAV password (its ADR-0005). The options
were the desktop keyring (Secret Service), an environment variable, or a file.

## Decision

We will keep the address and token in `~/.config/home/account.toml`, written with mode 0600, apart
from `config.toml`, which holds only settings. The user enters them in File → Account….

## Consequences

- Works the same everywhere, with no keyring daemon needed, and matches Calendar.
- The token is plaintext on disk, protected only by file permissions and disk encryption.
- Test and screenshot runs use a scratch `XDG_CONFIG_HOME` and never see the real token.
