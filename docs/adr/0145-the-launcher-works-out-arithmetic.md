# 0145. The launcher works out arithmetic

Date: 2026-11-16

## Status

Accepted

## Context

Spotlight answers `1 + 5` in its search box. The launcher's search box only listed apps and
files, so a quick sum meant opening a calculator.

## Decision

- **A small evaluator in the launcher** (`launcher/src/calc.rs`, no dependency): numbers,
  `+ - * / % ^`, `x` and `×` for times, `÷`, parentheses, unary minus, `pi` and `e`, and `sqrt`,
  `abs`, `sin`, `cos`, `tan`, `ln`, `log`, `round`, `floor`, `ceil`.
- **The answer is the first row**, above the apps, with the sum under it. Enter or a click copies
  the answer with `wl-copy` and closes the launcher. wl-copy is used instead of egui's
  `copy_text` because the launcher closes straight after, and a clipboard owned by a closing
  surface is lost.
- **Only what is worked out counts.** A bare number, a bare constant, `1 / 0` and anything
  unfinished give no row, so `7zip` or `e` still search apps and nothing sits above them.
- Ten significant digits, so `0.1 + 0.2` reads `0.3`.

## Consequences

- Sums are one keystroke away wherever the launcher is.
- Needs `wl-copy` to copy; without it the answer shows but Enter only closes, with a message on
  stderr.
- No units, currencies, variables or history; a calculator app is still the place for those.
