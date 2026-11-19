# 0013. The list's columns can be resized

Date: 2026-11-19

## Status

Accepted. The user asked for resizable columns in the list view; the gesture details are my design.

## Decision

- **Drag a column's left edge in the header** to resize it (resize cursor on hover). Name has no
  handle: it takes what the others leave, so the edge stays under the pointer.
- **Double-click the edge** to return the column to its default width.
- **Widths are kept** in `config.toml` (`column_widths`, by heading), at least 50 points; a
  column that would leave Name under 180 points is still dropped, as when the window is narrow.
