# 0013. The markdown model and the WYSIWYG widget move to Patina

Date: 2026-10-16

## Status

Accepted

## Context

Reader, a read-only viewer for a project's markdown and Rust files that is to become Patina's
help viewer, should show documents exactly as Notebook does. The widget (ADR-0002), the model,
the embeds (ADR-0005, ADR-0006, ADR-0012) and the fonts (ADR-0003) lived in this app.

## Decision

`markdown.rs`, `doc.rs`, `editor.rs`, `embeds.rs` and `fonts.rs`, with the fonts and their tests,
move to Patina as `patina::markdown` (Patina's ADR-0015), behind its `markdown` feature, with a
read-only mode added for Reader. Notebook depends on Patina's `develop` branch with that feature
and keeps the app: the window, workspaces, notes on disk, config.

## Consequences

- A change to how notes look or edit is made in Patina and reaches Notebook with
  `cargo update -p patina-ui`; Reader and help follow it too.
- Notebook builds from Patina's develop branch rather than a release.
- The decisions above stay recorded here; the moved code cites them as Notebook's ADRs.
