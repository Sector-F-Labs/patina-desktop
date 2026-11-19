# 0003. Cells are stored as the text that was typed

Date: 2026-11-05

## Status

Proposed

## Context

A sheet holds numbers, text and formulas. The schema could give every sheet its own SQL table with
typed columns, which is easy to query but makes inserting columns, mixed-type columns and
formulas awkward. Or it could be one generic table of cells.

## Decision

We will keep two tables: `sheets (id, name, position)` and
`cells (sheet_id, row, col, input)`, keyed by `(sheet_id, row, col)`. `input` is exactly what
the user typed. Text starting with `=` is a formula, text that reads as a number is a number,
anything else is text. Empty cells have no row. Values are worked out from the inputs when
a sheet is loaded and after each edit, and aren't stored.

## Consequences

- One simple schema holds any sheet. Formulas survive a round trip exactly.
- Plain SQL sees inputs, not results: `SELECT SUM(CAST(input AS REAL))` works on number cells but
  not on formula cells. If other tools need results, a `value` column written by Ledger is the
  next step, and a new ADR.
- Column widths, number formats and styles have nowhere to live yet. They'd be new tables.
- Cross-sheet references aren't supported (ADR-0005), so sheets are independent.
