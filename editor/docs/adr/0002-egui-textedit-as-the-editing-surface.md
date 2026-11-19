# 0002. egui's TextEdit is the editing surface

Date: 2026-10-25

## Status

Proposed

## Context

An editor needs a cursor, selection, keyboard movement, the clipboard, input methods, undo and
accessibility. egui's `TextEdit` has all of them, and takes a layouter, so the text can be
coloured. Its costs: it lays out the whole text as one galley, so every edit re-lays out the
whole file, and its undo keeps whole copies of the text.

The alternative was a widget of our own over a rope (ropey or similar), laying out only the lines
on screen: right for multi-megabyte files, but a large piece of work, and every piece of text
handling egui already has would have to be redone, input methods hardest of all.

## Decision

We will edit with `TextEdit::multiline`, given:

- a layouter that keeps the laid-out galley until the text, colours, width or marks change, so
  frames without edits cost almost nothing;
- line numbers and the cursor's line painted beside and behind it from its galley;
- Tab, Shift+Tab and Enter taken before it sees them, for indenting (`indent.rs`);
- Undo and Redo in the Edit menu through its own undoer, and `view::checkpoint` before changes the
  program makes (Replace All, a file read again), so those can be undone too.

## Consequences

- Typing, selection, the clipboard and input methods behave as in every other egui field.
- Files of a few megabytes get slow to edit; above 4 MB they're shown without highlighting
  (`highlight::MOST_BYTES`). Very large files are a reason to revisit this.
- Undo holds whole copies of the text, which matters only for large files.
