# 0107. Reader and Notebook live in this repository

Date: 2026-11-06

## Status

Accepted

Extends [0106](0106-files-lives-in-this-repository.md) to Reader and Notebook.

## Context

The same reasons as Files (ADR-0106). Process Manager opens man pages in Reader and Notebook is
the desktop's notes app; both were built from commits pinned in `bundle.toml`, so a change
that touched one and the desktop took two commits in two repositories.

## Decision

- Reader and Notebook are the `reader/` and `notebook/` crates of this workspace, with their
  history (`git subtree`) and their own ADRs, `AGENTS.md` and skills.
- `make stage` installs them as it does Files: `reader` and `notebook` are in `BINARIES`, with
  their launcher entries and icons. `bundle.toml` no longer lists them.
- Their own `make install` (per user) still works from their folders.

## Consequences

- `make deb` and `make install-full` build them from this repository's commit.
- The apps left in `bundle.toml` are Teleprinter, Draw, Montage and UnAmp.
