# 0108. The launcher lives in this repository

Date: 2026-11-06

## Status

Accepted

Changes [0037](0037-patina-shell-and-the-launcher-are-public.md), which moved the launcher out.

## Context

ADR-0037 gave the launcher a public repository of its own so it could be used on GNOME,
without the desktop. That still holds: it needs only a Wayland compositor and builds alone. But
the cost was a second repository the desktop's `make build` had to `cargo install` from GitHub,
a pin in `bundle.toml`, and a checkout beside this one for Patina OS's build scripts. Changes
to the bar's launcher button and to the launcher were made together (the skin and edge it
is passed).

## Decision

- The launcher is the `launcher/` crate of this workspace (package `patina-launcher`), with its
  history and ADRs (`launcher/docs/adr/`, numbered as they were). It uses the workspace's
  `patina-ui` and `patina-shell`.
- `patina-launcher` is in the Makefile's `BINARIES`: `make build` builds it with the rest and
  `make stage` installs it. `make build` no longer runs `cargo install --git`. `bundle.toml` no
  longer pins it, and `build.py` has no step for it.
- `make install` in `launcher/` still installs it alone into `~/.local`, and its README says to
  clone this repository for that.
- patina-shell stays a repository of its own.

## Consequences

- One commit changes the launcher and the desktop; `make check` covers both.
- Patina OS's and the FreeBSD scripts take the launcher from the desktop's build.
- The old repository, Sector-F-Labs/patina-launcher, stays as it was; its README should point here.
