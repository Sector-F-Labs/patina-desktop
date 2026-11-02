# 0103. Alt+Tab leaves a fullscreen window that has no other tab

Date: 2026-11-02

## Status

Accepted

## Context

Alt+Tab steps along the bar's tabs (ADR-0027) and is the way out of a fullscreen game (ADR-0041).
With the game the only window open there is no other tab, so it stepped to the game itself and
did nothing: the way out led nowhere, and the desktop and bar stayed behind the game.

## Decision

- When a fullscreen window has the keyboard and the next tab is that same window, Alt+Tab
  minimises it to its tab (ADR-0084), which shows the desktop and the bar.
- The next Alt+Tab, with nothing focused, goes to the first tab, which restores the game.
- With another tab to go to, Alt+Tab is as before.

## Consequences

- A game in the only window can be left and returned to with Alt+Tab alone.
- Minimising an X11 game tells it it's iconic (`xwayland.rs`); Wine restores it on focus, as
  ADR-0084 relies on.
