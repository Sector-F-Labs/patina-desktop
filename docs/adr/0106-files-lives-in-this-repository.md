# 0106. Files lives in this repository

Date: 2026-11-06

## Status

Accepted

Changes how [0099](0099-one-deb-of-the-desktop-and-its-apps.md) gets Files.

## Context

Files was a repository of its own, which the `.deb` and `make install-full` built from a commit
pinned in `bundle.toml`. But it is the session's file manager and its file chooser (ADR-0092's
portal config names it), and changes to it and to the desktop were always made together: a bump
of the pin, a second commit, a second repository to keep checked out beside this one.

## Decision

- Files is the `files/` crate of this workspace. Its history came with it (`git subtree`), and
  its ADRs stay in `files/docs/adr/`, numbered as they were.
- `make stage` installs it like Settings and Process Manager: the binary (`files` is in
  `BINARIES`), its launcher entry and icon, the D-Bus service for the portal backend, and
  `patina.portal`. `bundle.toml` no longer lists it.
- Files' own `make install` (per user, into `~/.local`) and `make app` (a `Files.app` folder)
  still work from `files/`.
- The old repository, Sector-F-Labs/files, stays as it was; its README should point here.

## Consequences

- One commit changes Files and the desktop; `make check` covers both.
- `make deb` and `make install-full` build Files from this repository's commit, not a pin.
- The website's Files page, and Patina OS's build scripts, read `../patina-desktop/files`.
