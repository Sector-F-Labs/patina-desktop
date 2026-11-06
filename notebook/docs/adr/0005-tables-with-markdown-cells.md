# 0005. Embed tables as grids of cells holding inline markdown

Date: 2026-10-11

## Status

Accepted (carried over from the GTK repo's ADR-0005, re-implemented in egui)

## Context

Tables are block widgets inside the editor's flow. The GTK version embedded a grid of text fields
whose cells hold the cell's inline markdown source.

## Decision

- **The widget:** a top-level table is a `Para::Table` drawn by `embeds::table`, a grid of
  single-line `TextEdit`s, one per cell. Each holds that cell's inline markdown (with `\|`
  unescaped), and the header row is drawn in the strong text colour.
- **Commands:** a cell's right-click menu inserts and deletes rows and columns, aligns a column,
  and deletes the table. "+ Row" and "+ Column" buttons sit under the grid.
- **Inserting:** from the toolbar's table button, as a 2×2 table.
- **Undo:** cell edits are undoable along with the rest of the note.
- **Not carried over:** Tab moves between cells the way egui moves focus, but Enter doesn't add a
  row at the end yet.

## Consequences

- **Markdown in cells:** inline formatting in cells shows as markdown, e.g. `**bold**`, the same
  trade-off the GTK version made.
- **Not first-class:** tables inside lists or quotes stay raw ([ADR-0004](0004-keep-unsupported-markdown-verbatim.md)).
- **Fixed width:** cells are 140 px wide, so long cell text scrolls inside its field.
