# 0004. Edits are saved as they're made, off the UI thread

Date: 2026-11-05

## Status

Proposed

## Context

Patina's rule is that file I/O never blocks the UI. A spreadsheet's file is also a database,
which already commits atomically, so the "Save" step of a document app can go away entirely.

## Decision

We will have no Save command. Opening a file reads all of it into memory. Each edit is sent to a
writer thread that owns the connection, which commits everything queued so far in one
transaction. Closing the file, or quitting, waits for the queue to finish. A failed save is
shown in the status bar.

## Consequences

- There is nothing to forget to save, and no unsaved state to lose in a crash beyond the last
  few queued edits.
- There is no undo yet, so a mistake is only fixed by retyping. Because edits are written at once,
  undo has to be an app feature (a stack of edits) rather than "close without saving".
- A big file is read in full on open, on the UI thread. Fine for books; revisit if a sheet gets
  to hundreds of thousands of cells.
- A second Ledger writing the same file isn't noticed (ADR-0002).
