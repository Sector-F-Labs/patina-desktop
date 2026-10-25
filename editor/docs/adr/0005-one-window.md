# 0005. One window: a second start hands its files to the first

Date: 2026-10-25

## Status

Proposed

## Context

Opening a file from Files, or `text-editor FILE` in a terminal, while Text Editor is open should
open it as a tab there, as GNOME's does. Two independent windows would also each keep the session
(ADR-0004), and the last to quit would overwrite the other's.

GNOME's apps do this through D-Bus activation (`GApplication`). That would need a bus name and a
D-Bus service file; a Unix socket needs neither.

## Decision

The first Text Editor listens on a socket in `$XDG_RUNTIME_DIR` (`instance.rs`), named for the data
folder, so a test run with a scratch data folder is a window of its own. A later start connects,
sends its files a line each, and exits; the first opens them and asks to come forward. A socket
nobody answers on, left by a crash, is replaced.

## Consequences

- One window and one session per user (per data folder).
- There's no New Window; tabs are the way to have several documents.
- Being brought forward depends on the compositor: on Wayland, `ViewportCommand::Focus` without an
  activation token may only mark the window as wanting attention.
- Unix only.
