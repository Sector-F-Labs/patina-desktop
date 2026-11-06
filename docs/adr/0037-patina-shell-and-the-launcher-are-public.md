# 0037. patina-shell and the launcher are public repositories of their own

Date: 2026-10-16

## Status

Superseded by [0109](0109-patina-shell-lives-in-this-repository.md). The launcher part was changed by [0108](0108-the-launcher-lives-in-this-repository.md): it is back in this repository.

## Context

The launcher had become useful outside the Patina desktop: on GNOME it opens as a centred window,
like wofi, from a key bound in GNOME or wbindkeys. To publish it, its dependencies had to be
public too. patina-apps (folder apps) was a private repository; patina-shell (the layer-shell
host it's drawn on) lived inside this private repository, mixed with desktop-specific code
(NetworkManager, wpctl, logind, the battery, the tray's icons). Copying the host into the
launcher would have meant two thousand-line hosts drifting apart; making the whole desktop
public was more than was wanted. The user chose GitHub and git dependencies over crates.io.

## Decision

- The desktop-specific modules move to **patina-system** (`system/`), which stays here.
- **patina-shell** keeps the generic half (the host and its window fallback, the painter,
  scaling, one copy per display, the bar's skin and edge, the window list) and moves, with its
  history, to the public repository Sector-F-Labs/patina-shell. The workspace depends on it by
  git.
- **patina-launcher** moves, with its history and its ADRs (0010, 0035, 0036 here, renumbered
  there, with a new one for the GNOME window), to Sector-F-Labs/patina-launcher. `make install`
  builds it from there with `cargo install --git`; the compositor finds it on PATH as before.
- **patina-apps** becomes public at Sector-F-Labs/patina-apps, so nothing here needs ssh to build:
  `.cargo/config.toml` goes.

## Consequences

- The launcher and the layer host are usable by anyone, on any Wayland desktop.
- A change to patina-shell is made in its repository and pulled here with
  `cargo update -p patina-shell`; working on both at once takes a local `[patch]`.
- `make run` no longer builds the launcher; the installed one is used.
- Patina OS's image build has to build the launcher from its repository too.
