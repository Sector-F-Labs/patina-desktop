# 0005. A small formula evaluator of our own, within one sheet

Date: 2026-11-05

## Status

Proposed

## Context

Bookkeeping needs totals and simple arithmetic: `=D2+E2`, `=SUM(D2:D9)`, `=ROUND(D2*0.25, 2)`.
Crates that evaluate spreadsheet formulas exist, but they bring a larger language than we need,
and with it spreadsheet behaviour (coercions, dates, locale) we'd have to match or explain.

## Decision

We will write the evaluator in `src/formula.rs`: `+ - * / ^ &`, unary minus, parentheses,
cell and range references, string literals, and `SUM`, `AVERAGE`, `MIN`, `MAX`, `COUNT`,
`ABS` and `ROUND`. Each cell is evaluated once per recalculation, a cell that depends on itself
shows `#CYCLE`, and errors (`#DIV/0`, `#VALUE`, `#NAME`, `#PARSE`) show in the cell instead of
stopping anything. References stay within the sheet. Numbers are `f64`, shown rounded to ten
decimals.

## Consequences

- We control the behaviour and it's easy to test. A bad formula can't crash the app.
- `f64` is wrong for money in the long run: sums of many cents drift, and `ROUND` is the user's
  tool for that. A decimal type is the first thing to revisit, since it changes the stored
  results' meaning (and would be a new ADR).
- No `IF`, comparisons, dates, cross-sheet references (`Income!D2`) or relative copy of formulas
  when pasting. Pasting copies the values.
