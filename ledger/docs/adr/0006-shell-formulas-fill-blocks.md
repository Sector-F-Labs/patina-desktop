# 0006. Shell formulas fill a block of cells, and only run in trusted ledgers

Date: 2026-11-05

## Status

Proposed

## Context

The user wants formulas that run programs from `PATH`, because many programs print columns
(`df -h`, `ps`, `docker ps`) and bookkeeping often starts from a command's output. The design is
in `~/Notes/Projects/Ledger shell formulas.md`. The user decided that the formula lives in the
top-left cell of the block it fills, and that output that doesn't fit its block must be marked on
the block's edge. This is a prototype to try the idea, so some answers are the simplest that work.

A ledger file used to be inert data. With this it can start programs, and a ledger may be
emailed to someone.

## Decision

We will add `=SH(command, range?, options...)`:

- **Where it goes:** the output fills a block starting at the formula's own cell. The range is
  optional: without one the block is as big as the output. `:F20` ends at column F and row 20,
  `:F` ends at column F with no end row, and `B2:F` is the same written out (its start must be the
  formula's cell). An end row cuts off lines; with `rest=drop` an end column cuts off fields.
- **Options:** `sep="…"` (default: runs of whitespace), `skip=N` lines, `rest=keep|drop` (default
  `keep`: the last column takes the rest of the line), `timeout=seconds` (default 10), and one
  `arg=expr` per positional argument.
- **Running:** `sh -c command sh arg1 arg2 …` in its own process group, with no stdin, on a
  worker thread. Cell values are only ever arguments (`$1`, `$2`), never part of the command's
  text. A timeout kills the whole group. Output is cut off at 4 MiB.
- **When it runs:** when the formula is entered, on Sheet → Run Shell Cells (Ctrl+R), and when
  the ledger is first trusted. Opening a file never runs anything: it shows the stored results.
  Changing a cell the command reads does not re-run it.
- **Trust:** a ledger's shell cells run only if its real path is in `trusted` in
  `~/.config/ledger/config.toml`, set by File → Trust This Ledger. The file can't trust itself.
  Without trust the cells show their last results and the status bar says why.
- **Storage:** format 2 adds `shell_results (sheet_id, row, col, formula, status, stdout,
  stderr, ran_at)`. The raw output is stored, and the block is worked out from it again on every
  load, so splitting options can change without re-running. A result whose `formula` differs from
  the cell's current input is stale and ignored. Opening a format 1 file adds the table and sets
  `user_version` to 2.
- **Values:** the block's cells are derived: they're shown (tinted), and formulas such as
  `=SUM(C3:C9)` read them, but they have no stored input. A block that would cover typed cells or
  an earlier block is refused and its formula cell shows `#SPILL`; nothing is ever overwritten.
  Other errors show as `#NORUN`, `#EXIT`, `#TIMEOUT`, `#FAILED`, `#RANGE`, and `…` while running.
- **Cut-off output:** the edge where something was cut is drawn heavier in the skin's warning
  colour with a `…` on it; hovering explains how much was cut and how to fix it.

## Consequences

- Plain SQL tools can read the raw output, and the ledger can be reopened with no command run.
- This is the first schema migration. It's one-way in practice: a format 1 Ledger refuses a
  format 2 file as "made by a newer Ledger" (ADR-0002), so a file opened by this version can't go
  back to an older one.
- Trust is per file, not per command, and there's no sandbox: a trusted ledger can run anything
  the user can. Trusting a file whose formulas the user hasn't read is the user's risk.
- Whitespace splitting breaks fields that contain spaces, and `sep` doesn't understand CSV
  quoting. A real CSV reader is the next step for `jq @csv` and similar.
- Results go stale silently when the cells a command reads change. A dependency-aware re-run, or
  an indicator that a result is older than its inputs, is the obvious follow-up.
- A command can't read a range as input yet. Each run is a thread, with no limit on how many.
- `SH` is only valid as a whole formula, not inside another one (`=SUM(SH(...))` is `#NAME`).
- Blocks are drawn but not yet selectable as a unit, and typing into a derived cell is allowed:
  it makes the formula's block conflict (`#SPILL`) until the typed cell is cleared.
