# 0004. Read dwl's status from stdin

Date: 2026-10-14

## Status

Proposed

## Context

Stock dwl reports its state by printing lines (`<output> tags 7 1 1 0`, `<output> title ...`) to
stdout after every change, piped into the `-s` startup command's stdin. Waybar's dwl modules
instead need the dwl-ipc patch (`zdwl_ipc_manager_v2`), which also lets a bar switch tags. If the
`-s` command stops reading, dwl blocks once the pipe fills.

## Decision

We will run as `dwl -s patina-hud` and parse dwl's status lines from stdin (`src/dwl.rs`), on a
thread of its own that never waits on drawing. Each output's bar shows the monitor dwl names after
it; an output dwl hasn't named shows the selected monitor. Unknown fields are ignored, so patched
dwls that print more still work.

## Consequences

- Works with unpatched dwl, as packaged by FreeBSD and the Linux distributions.
- The channel is one-way: clicking a tag does nothing yet. Switching tags from the bar needs the
  dwl-ipc patch, keybinding emulation, or our own dwl fork; that will be a new ADR.
- Only one bar process can be dwl's `-s` command.
