# 0044. Smithay comes from a fork while 0.7 names the root window as the active one

Date: 2026-10-17

## Status

Accepted

## Context

Skyrim Special Edition, through Proton under Xwayland (ADR-0040), drew a blank screen with its
music playing. On another machine the game was tiny in the top-left corner of a black screen. The
same install works under GNOME on the same machine. Wine's log showed the cause. Smithay 0.7's X11
window manager watches focus on the root window and writes each FocusIn and FocusOut's event window
into `_NET_ACTIVE_WINDOW`, and that is the root itself. Wine reads the property, decides its
fullscreen game has lost focus, and minimises it. Minimised, Wine shrinks the child window the game
renders into to 1×1, while the game runs on. Mutter names the focused window.

Upstream fixed this on 2026-10-06: focus changes are selected on each window, a FocusIn on a
managed window names it, and focus going back to the root clears the property. The fix isn't
released. Moving to Smithay's master branch was tried and ruled out: it breaks most of the
compositor (the `delegate_*` macros are gone, winit changed) for one fix. Vendoring a patched 0.7
into this repository (4.4 MB, with its warnings in every build) was ruled out for the fork.

## Decision

- Sector-F-Labs/smithay is a fork of Smithay. Its `patina-0.7` branch is the 0.7.0 tag plus the
  backported focus fix as one commit.
- The workspace's `Cargo.toml` replaces crates.io's smithay with that branch in `[patch.crates-io]`,
  so the compositor's dependency still reads `version = "0.7"`.
- Further fixes Patina needs from upstream before a release go on that branch as commits on 0.7.0.

## Consequences

- Fullscreen Wine games stay focused, and render.
- Another public repository to keep. Fixes there are backports, and each one has to be redone
  against upstream's API when we move to the next release.
- When a Smithay release has the fix, the compositor moves to it, the `[patch.crates-io]` goes,
  and this is superseded. That move means following master's API changes.
