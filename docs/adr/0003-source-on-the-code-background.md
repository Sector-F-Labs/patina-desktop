# 0003. Source files on the skin's code background

Date: 2026-10-16

## Status

Accepted

## Context

Source on the plain panel background looked like prose. The user asked for source files to have
the skin's slightly darker background, the one code blocks in markdown have (in Steam Classic,
its darker green).

## Decision

A source file is drawn in a frame filled with the skin's `code_bg_color`, rounded, filling the
content area, with the code scrolling inside it both ways.

## Consequences

- Code reads as code in every skin, matching code blocks in documents.
- Man pages and markdown stay on the panel background, as documents.
