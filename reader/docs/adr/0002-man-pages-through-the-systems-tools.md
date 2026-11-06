# 0002. Man pages, formatted by the system's man tools

Date: 2026-10-16

## Status

Accepted

## Context

The user asked for man pages to be readable too: projects ship them (`wbindkeys.1`), and help for
command-line tools is a man page. Their source is roff, which takes a formatter to read.

## Decision

- Files named for a man section (`.1` to `.9`, perhaps with letters, perhaps gzipped) are shown
  as man pages; Go → Man Page opens an installed one by name through `man -w`.
- The system formats them: `mandoc -T utf8` where there is one (FreeBSD's base system), else
  man-db's `man -l`, at 80 columns. Both mark bold and underline with backspace overstrikes,
  which Reader parses into styles and draws in the skin's colours (bold in the strong text
  colour, underline in the link colour), in the monospace font, selectable.

## Consequences

- Pages look as `man` shows them in a terminal, in the skin, with no roff parser of our own.
- Without mandoc or man installed, a man page shows an error instead.
- The page isn't reflowed to the window's width; it stays at 80 columns.
