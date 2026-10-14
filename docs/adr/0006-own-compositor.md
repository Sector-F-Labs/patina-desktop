# 0006. Build our own compositor on Smithay

Date: 2026-10-14

## Status

Accepted

## Context

patina-bar first ran on dwl (ADR-0004, ADR-0005). Making its tags clickable needed either the
dwl-ipc patch, key emulation through a virtual keyboard, or a fork of dwl. dwl is small and its
policy is its own: it ignores `xdg_toplevel.move` and `resize`, so Patina's frame can't be dragged,
and maximise and minimise do nothing. The user decided not to get attached to dwl and to move to a
compositor of our own, step 6 of the "Patina desktop" plan, sooner than planned. The key-emulation
work was shelved.

dwl is C on wlroots. Smithay is a Rust library for compositors, used by niri and cosmic-comp, and
runs on FreeBSD (niri is in its packages).

## Decision

We will write patina-compositor in Rust on Smithay. The first milestone is only to draw foot and
patina-bar: xdg-shell windows and layer-shell surfaces, a seat with keyboard and pointer, and
click to focus. The bar stays a separate layer-shell client, not part of the compositor.

## Consequences

- Window policy, borders and IPC are ours to decide, so Patina's frame can be honoured.
- A compositor is far more code to own than a bar; Smithay supplies the protocols, not the policy.
- The bar's dwl modules (tags, layout) show nothing on our compositor until it has workspaces,
  which the bar will learn together with the compositor.
