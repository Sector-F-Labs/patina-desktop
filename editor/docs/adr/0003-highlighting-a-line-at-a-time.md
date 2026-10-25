# 0003. Syntax highlighting with syntect, a line at a time and kept between edits

Date: 2026-10-25

## Status

Superseded by [ADR-0010](0010-the-highlighter-lives-in-patina.md)

## Context

Patina's code blocks (and Reader's source view) highlight through `egui_extras`, which runs
syntect over the whole text whenever it changes. That's fine for a code block, but in an editor it
means re-highlighting the whole file on every keystroke, which stalls typing in files of a few
thousand lines.

## Decision

We will drive syntect ourselves (`highlight.rs`): each line keeps the parser's state at its start
and its coloured spans. An edit finds the lines it touched (by the common prefix and suffix of the
old and new text), and colouring goes again from the first of them only until a line's starting
state is what it was before; the lines after it keep their colours. A frame spends at most 4 ms
colouring, so a change that recolours the rest of the file (opening a block comment) catches up
over a few frames, the lines not reached yet keeping their old colours meanwhile.

The themes are the ones Patina's code blocks use, base16-mocha in dark and Solarized (light), so
code looks the same across Patina apps. Text in the theme's own foreground colour is drawn in the
skin's text colour instead, so plain text and unscoped tokens follow the skin.

Languages come from the file's name, else its first line (`#!/bin/sh`), and can be chosen by hand
from the status bar; a choice is remembered with the tab.

## Consequences

- Typing stays immediate in long files, and the colouring is tested to match colouring from scratch
  after every kind of edit.
- syntect is a direct dependency, kept to the version and features `egui_extras` uses.
- Token colours come from syntect's themes, not the skin; a skin can't recolour keywords yet. An
  `[app.text-editor]` skin section mapping scopes to colours would be the way to add that.
