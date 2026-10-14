# 0005. Test on dwl in a bsdt FreeBSD VM

Date: 2026-10-14

## Status

Accepted

## Context

The bar needs a wlr-layer-shell compositor to run against, and the user's desktop is GNOME, which
has none. Running dwl nested is possible, but a VM gives a clean, repeatable desktop that also
proves the bar on a BSD. bsdt (the user's dev-VM tool) can run any wlroots compositor headless and
share it over VNC (`[gui] desktop = "wayland"`).

## Decision

We will test in a FreeBSD 15.1 VM from `bsdt.toml`: dwl, foot, wmenu and grim from packages, the
bar built in the VM by `provision`, and dwl started as `dwl -s 'exec patina-bar/target/release/patina-bar'`.
`make vm-restart` rebuilds and restarts the desktop; `make screenshot` takes the bar with grim.

## Consequences

- Every change is checked on FreeBSD, so Linux-only APIs show up early.
- dwl comes from FreeBSD packages with its default config, so its keys are dwl's defaults (Alt as
  the modifier, Alt+Shift+Return for a terminal).
- The VM renders in software, so it says nothing about GPU or HiDPI behaviour.
