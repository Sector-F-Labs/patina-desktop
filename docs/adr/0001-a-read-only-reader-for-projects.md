# 0001. A read-only reader for a project's markdown and Rust, on Notebook's widget

Date: 2026-10-16

## Status

Accepted

## Context

The user wanted to point an app at a project, such as the Patina desktop, and browse its code and
documentation, and later use it as Patina's documentation reader when people choose Help in an
app. Notebook already shows markdown the way Patina apps should; an editor would be the wrong
tool for reading, and a browser renders markdown its own way.

## Decision

- Reader shows two kinds of file, markdown (`.md`) and Rust (`.rs`), and only reads them.
- Markdown goes through Notebook's widget, moved into Patina as `patina::markdown` for this
  (Patina's ADR-0015), in its read-only mode: selection and copying, no edits, and links
  followed by Reader: another readable file opens in Reader, a folder its README, a URL the
  browser, anything else the desktop's app for it.
- Rust is shown highlighted with line numbers (`patina::markdown::embeds::code_file`).
- A project is a folder; the tree holds only what Reader shows, skipping build output, version
  control and hidden folders, with READMEs first. A file given on the command line opens its
  repository (the nearest folder with a `.git`).
- It starts in the Notebook skin, and remembers the last project and file.

## Consequences

- Documents read in Reader look exactly as they do in Notebook, and will in help.
- Other languages and file types aren't shown until a reason comes; adding one is a `Kind` and
  a view.
- Links to a heading (`#section`) open the file at its top: the widget doesn't scroll to
  headings yet.
