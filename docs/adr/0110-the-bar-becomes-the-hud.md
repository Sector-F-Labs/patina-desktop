# 0110. The bar becomes the HUD

Date: 2026-11-06

## Status

Accepted

Extends [0063](0063-split-bar.md).

## Context

The bar began as a strip along an edge, a copy of waybar's layout (ADR-0001). The way it is
used now is the split layout (ADR-0063): the launcher, the window list and the tray each on a
surface of their own, where the user puts them. That is the shape the desktop is going to
have, and each element needs to be configurable on its own: hidden altogether, or kept off
screen until the pointer goes where it was put. "Bar" no longer describes a thing that is one
strip.

## Decision

- The crate, directory and binary are `hud`, `hud/` and `patina-hud`. `PATINA_BAR_EDGE` is
  `PATINA_HUD_EDGE`, `make run-bar` is `make run-hud`, and the man page is patina-hud(1).
- The config folder is `~/.config/patina-hud/` (the file and `skins/` in it). A user whose
  folder is still `~/.config/patina-bar/` has it moved on first read, by
  `patina_shell::hud::config_path()`, which every piece uses.
- patina-shell's `bar` module is `hud`.
- ADRs written before this one keep saying "the bar"; they record what was decided then.

## Consequences

- Anything outside this repository that runs `patina-bar` or sets `PATINA_BAR_EDGE` has to
  change. Within the Sector F repositories the installer and patina-os' build are updated.
- Settings' "Bar" page keeps its name for now.
