# 0058. patina-raise: focus an app's window, or start it

Date: 2026-10-17

## Status

Accepted. The user asked for a way to launch or select an app: Alt+B for Firefox should focus an
open Firefox window, and start Firefox only if none is open. How it matches changed in
[ADR-0060](0060-patina-raise-matching.md).

## Context

The compositor's `[bindings]` (and wbindkeys, the user's own key binder) run commands. Alt+B ran
`firefox`, which opened another window each time, though Firefox was usually already open. This
"run or raise" is common: GNOME has extensions for it, and i3 users script it.

The command has to know which windows are open, and be able to focus one. Two ways were there:

- **A method on the compositor's session bus.** This would be Patina-only.
- **wlr-foreign-toplevel-management**, which the bar already uses for its tabs (ADR-0012). It
  lists windows with their app ids and can activate one. sway, Hyprland and other wlroots
  compositors serve it too, which fits wbindkeys' aim of working under any compositor.

## Decision

- **A small program, `patina-raise APP [-- COMMAND ...]`** (`raise/`).
  - It lists the windows over wlr-foreign-toplevel, with plain wayland-client.
  - It focuses APP's window, matched by app id ignoring case, or by the last part of a
    reverse-DNS id.
  - If one of APP's windows already has the focus, it focuses the next one, round to the first.
  - With none open, it runs COMMAND (or APP), through the compositor's `Spawn` where it can.
  - `--list` prints the open windows' app ids.
  - `choose.rs` holds the decision, with its tests.
- **Alt+B defaults to `patina-raise firefox`.** Alt+T stays `teleprinter`, which opens a new
  terminal each time, as terminals usually do.

## Consequences

- One key goes to an app whether or not it's running, and steps through its windows.
- It works from any key binder, under Patina or another wlroots compositor. Under GNOME, which
  doesn't serve the protocol, it only ever starts the app.
- Matching is by app id, which for a few apps isn't their name. `--list` shows what to use.
- A window that's minimised or on another output is still focused, as the compositor does when
  activating.
