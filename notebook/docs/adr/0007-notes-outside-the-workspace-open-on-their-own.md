# 0007. Open notes from outside the workspace on their own

Date: 2026-10-11

## Status

Accepted (carried over from the GTK repo's ADR-0008, minus single-instance launching)

## Context

Notebook opens a folder as its workspace, but a note can also be opened directly, from the command
line or a file manager. The GTK version opened such a note inside the workspace containing it, or
on its own if no workspace contained it.

## Decision

`notebook <path>` opens:

- **a folder** as the workspace;
- **a note inside the remembered workspace** inside that workspace;
- **any other note** on its own. The sidebar says so and offers "Open Containing Folder" and
  "Back to <remembered folder>". The remembered folder isn't changed.

Single-instance launching is not carried over: each launch is a new window.

## Consequences

- Opening a stray `.md` file doesn't replace the workspace.
- Without single-instance launching, opening notes from the file manager starts more windows.
  Bring it back if that gets annoying in practice, e.g. over D-Bus.
