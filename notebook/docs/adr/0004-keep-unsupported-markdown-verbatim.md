# 0004. Keep markdown the editor can't represent as verbatim raw text

Date: 2026-10-11

## Status

Accepted (carried over from the GTK repo's ADR-0003)

## Context

Saving rewrites the whole note from the document model. Anything the model can't represent would
be lost, unless it's carried through untouched.

## Decision

As in the GTK version, unchanged:

- **Unsupported blocks** become `Raw` lines holding their exact source: HTML, footnote
  definitions, front matter, complex lists, and tables or code inside lists and quotes. They're
  shown in grey monospace, editable as text, and written back as they are.
- **Unsupported inline constructs**, such as images and inline HTML, become `Raw` spans the same
  way.
- **The code:** this is `markdown.rs`, carried over unchanged, and covered by its round-trip tests
  and by an editor test.

## Consequences

- Opening and saving a note never drops content.
- Raw text is still markdown on screen, the one place syntax shows.
- Each construct the editor learns to represent moves out of `Raw`, in both directions of the
  conversion.
