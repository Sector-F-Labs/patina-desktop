# 0002. Edit notes in a WYSIWYG rich-text widget of our own

Date: 2026-10-11

## Status

Accepted

## Context

Notebook edits notes like Apple Notes, with no markdown visible (the GTK repo's ADR-0002). GTK did
this with `GtkTextView` tags and child anchors. egui has no rich-text editor: `TextEdit` edits a
single string, and a layouter can style it but can't hide characters from the cursor.

The user weighed three ways to edit:

- **Block editor**, Notion-style. Each paragraph is its own `TextEdit`, with block syntax hidden
  but inline markers (`**`, `_`) drawn faint beside the text. Moderate effort.
- **Live preview**, Obsidian-style. One `TextEdit` holds the real markdown, styled in place. The
  least code, but the markdown stays visible, which the GTK ADR-0002 had ruled out.
- **True WYSIWYG**: a widget of our own with no markdown on screen at all. The most work and the
  most risk.

The user chose true WYSIWYG.

## Decision

`editor.rs` is a custom egui widget that edits the document model directly.

- **Model:** `doc.rs` turns the markdown model's lines into paragraphs: text lines, whole tables,
  and code blocks gathered into one paragraph each. It also holds the operations on a line's
  styled spans (insert, delete, split, slice, set a style), counted in characters.
- **Layout:** each text paragraph is one galley, built from its spans (size and weight per
  paragraph style, plus bold, italic, strike, code, link).
  - List markers, numbers, checkboxes and quote bars are painted outside the text, so the cursor
    never enters them.
  - Tables and code blocks are embedded widgets ([ADR-0005](0005-tables-with-markdown-cells.md),
    [ADR-0006](0006-code-blocks-highlighted-with-syntect.md)). The cursor skips over them.
- **Input:** while the editor has focus, it reads egui's events itself:
  - typed text and IME commits, with the composition shown underlined at the cursor;
  - movement and editing keys;
  - copy, cut and paste;
  - presses, drags and double/triple clicks on the text.

  Galleys from the last frame are used for hit testing and for moving up and down by row.
- **Behaviour carried over from the GTK version:**
  - Enter continues lists, and leaves a list or quote from an empty line.
  - Enter at the start of a line pushes it down.
  - Backspace at the start of a line first removes its paragraph style, then joins it to the line
    above.
  - Tab and Shift+Tab indent list items.
  - Clicking a checkbox toggles it.
  - Ctrl+click opens a link.
  - Formatting goes through toolbar and shortcuts (`toggle_inline`, `set_block`).
- **Undo:** snapshots of the whole document. Runs of typing within a second are joined into one
  step, up to 200 steps.
- **Clipboard:** the system clipboard gets plain text. The app remembers the paragraphs behind its
  last copy, so pasting that same text back keeps the formatting. Any other paste is plain text.
- **Markdown view:** a Markdown toggle (Ctrl+Shift+M) shows and edits the source in a plain
  `TextEdit`, as before.

## Consequences

- No syntax on screen, as in the GTK version, and the editor now follows the skin.
- **Ours to maintain:** every editing behaviour, the cursor, selection, IME and clipboard. The
  editor tests drive it through egui events, the same way it receives them in the app.
- **Missing:** accessibility (screen readers can't see into the widget), spell checking,
  bidirectional text, and rich paste from other apps.
- **Hidden typing styles:** toggling bold with nothing selected applies to what's typed next,
  which nothing on screen shows. The toolbar shows the pending style instead.
- **Undo memory:** each snapshot copies the whole note. Fine for notes; it would be a problem for
  very large documents.
- **Revisit** if IME composition, very long notes or accessibility become real problems. A
  piece-table model or AccessKit integration would be the next steps.
