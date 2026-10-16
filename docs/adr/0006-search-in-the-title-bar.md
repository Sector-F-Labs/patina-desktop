# 0006. Search the text on screen from the title bar

Date: 2026-10-16

## Status

Accepted

## Context

The user asked for search in the window's title bar that searches the text being shown. The tree
already had a filter for file names on Ctrl+F.

## Decision

- A search field sits at the right of the title bar, before the window buttons; Ctrl+F focuses
  it, and the tree's file filter moves to Ctrl+P.
- It finds the text, case aside, in the file on screen and marks every place; the current one is
  marked strongly and brought into view. Enter or F3 goes to the next, Shift+Enter or Shift+F3
  the previous, wrapping; Escape clears it. "3 of 12" beside it says where you are.
- Markdown is searched by Patina's editor (`Editor::set_search` and `find`, which select the
  match), man pages and source by Reader, marked through `patina::markdown::embeds::mark`. The
  search follows to the next file and tab while the field holds text.
- The field is drawn in the window's colours, not the bar's, so it reads on any skin's bar.

## Consequences

- Text in markdown tables and code blocks isn't searched yet: the editor searches its text
  paragraphs.
