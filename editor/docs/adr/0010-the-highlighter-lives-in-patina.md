# 0010. The highlighter lives in Patina

Date: 2026-10-25

## Status

Accepted. The user asked for the highlighter to move into Patina, so Reader's source and code
blocks match Text Editor. Supersedes [ADR-0003](0003-highlighting-a-line-at-a-time.md) and
[ADR-0009](0009-languages-from-bats-syntax-set.md), whose decisions now stand as Patina's
ADR-0027.

## Context

Text Editor's `highlight.rs` (syntect a line at a time, bat's languages through two-face, plain
text in the skin's colour) was wanted by Reader and by Patina's code blocks. Patina's rule is that a
component a second app wants moves into Patina, rather than being copied.

## Decision

The highlighter is `patina::highlight` (Patina's `highlight` feature), unchanged; Text Editor uses
it, and no longer depends on syntect or two-face itself. Its tests moved with it.

## Consequences

- Code looks the same in Text Editor, Reader and Notebook.
- Changing the highlighter means changing Patina, pushing `develop`, then `cargo update -p
  patina-ui` here.
