# 0005. --man: the system's man pages as the tree

Date: 2026-10-16

## Status

Accepted

## Context

Reader shows man pages that live in a project (ADR-0002) and opens an installed one by name.
The user asked for a system mode whose tree is all the man pages on the system and only those,
with the flag `--man`.

## Decision

- `reader --man [PAGE]` (and Go → System Manual) builds the tree from every folder on the
  manpath (`manpath`, else the usual ones): a folder per section, titled as man titles them
  ("1 User Commands"), holding each page by name; translations (`de/`, `ja/`) are left out, and
  where a page is in more than one folder the first on the manpath wins, as for `man`.
- It opens PAGE, found with `man -w` (`ls`, `printf(3)`), else man(1). Pages show by their man
  names (`ls(1)`) in the title and path line. Like single-file mode, nothing read here is
  remembered as the last project.
- Opening a project leaves man mode.

## Consequences

- The whole manual browses and filters like a project; reading ~7,400 page names takes a few
  tens of milliseconds, with nothing formatted until a page is opened.
- A section of thousands of pages is a long tree; the filter is the way to a page.
