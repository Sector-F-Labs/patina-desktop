# 0109. patina-shell lives in this repository

Date: 2026-11-06

## Status

Accepted

Supersedes [0037](0037-patina-shell-and-the-launcher-are-public.md); [0108](0108-the-launcher-lives-in-this-repository.md) already took the launcher back.

## Context

ADR-0037 gave patina-shell a public repository of its own because the launcher, published
alone, needed everything it builds on to be public. The launcher now lives here (ADR-0108), so
every crate that uses patina-shell is in this workspace. What was left was the cost: a second
checkout and an uncommitted `[patch]` to change it along with the desktop, a push and a
`cargo update -p patina-shell` before the desktop saw the change, and a pin on Patina's
`develop` of its own that could disagree with the desktop's.

## Decision

- patina-shell is the `shell/` crate of this workspace, with its history. The package keeps its
  name; the workspace depends on it by path.
- Someone outside can still depend on it by git: `patina-shell = { git =
  "https://github.com/Sector-F-Labs/patina-desktop" }` (cargo finds the crate in the workspace).
- The old repository, Sector-F-Labs/patina-shell, stays as it was; its README should point here.

## Consequences

- A change to the host and to what uses it is one commit, and `make check` covers both.
- Nothing in the desktop is built from a git dependency on one of its own parts any more; the
  git dependencies left are Patina, patina-apps and the Smithay fork.
