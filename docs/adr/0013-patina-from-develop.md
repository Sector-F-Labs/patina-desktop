# 0013. Depend on Patina's develop branch

Date: 2026-10-14

## Status

Accepted

## Context

The window tabs needed a change in Patina (`TabStrip::new_tab` and `noun`, Patina ADR-0012). The
desktop moves fast and will keep needing Patina changes; releasing Patina to crates.io for each one
is slow and public. A path dependency on `../patina` builds here but not in the test VM, which only
syncs this project.

## Decision

The workspace takes `patina-ui` from Patina's `develop` branch on GitHub
(`[workspace.dependencies]` in `Cargo.toml`), and the crates use `patina-ui = { workspace = true }`.
Patina changes the desktop needs go to Patina's `develop` first; releases go to crates.io from there
as before.

## Consequences

- A Patina change is usable here once it's pushed to `develop`, in the VM too.
- `Cargo.lock` pins a `develop` commit; `cargo update -p patina-ui` picks up newer ones.
- The desktop can't be published to crates.io while it depends on a git branch.
