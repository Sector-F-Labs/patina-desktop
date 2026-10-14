# 0012. The bar learns about windows through wlr-foreign-toplevel-management

Date: 2026-10-14

## Status

Proposed

## Context

The user asked for the open windows to be listed next to the launcher button, using Patina's tabs.
The bar is a separate client, so the compositor has to tell it which windows exist and let it
activate or close one. Smithay 0.7 implements `ext-foreign-toplevel-list-v1`, which only lists
windows: there's no way to activate one. `wlr-foreign-toplevel-management-unstable-v1` lists them
with title, app id and state, and has activate and close requests. Waybar's and sfwbar's taskbars
use it, and wlroots compositors implement it.

## Decision

The compositor implements `wlr-foreign-toplevel-management` v3 itself (`compositor/src/toplevels.rs`),
as a diff run after each event-loop pass: windows that opened, closed or changed title, app id or
activation are sent, nothing else. The active window is the one with the keyboard. Activate focuses
and raises a window; close asks it to close. Maximise, minimise, fullscreen and the taskbar rectangle
are ignored until the compositor has them.

patina-shell binds the protocol when `Options::toplevels` is set, and gives a `Shell` the list
(`toplevels_changed`) and takes its requests (`take_toplevel_requests`). The bar's `windows` module
shows the list as a `patina::tabs::TabStrip` without `+`, saying "Window" (Patina ADR-0012).

## Consequences

- Other taskbars (waybar's `wlr/taskbar`) work on this compositor too, and the bar's window list
  works on other wlroots compositors.
- The protocol is wlroots' unstable one; if `ext-foreign-toplevel` gains window management, revisit.
- Tabs keep the order windows opened in, not stacking order, so they don't jump around on focus.
