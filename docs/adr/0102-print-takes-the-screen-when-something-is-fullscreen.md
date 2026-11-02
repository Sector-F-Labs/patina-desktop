# 0102. Print takes the whole screen when something is fullscreen

Date: 2026-11-02

## Status

Accepted

## Context

The Print key opens patina-screenshot's choices (ADR-0091), which take the keyboard. Over a
fullscreen game that pulls the keyboard from it, and the choices are one more thing in the way of
the shot. Whether a window is a game can only be guessed; that it has the whole screen is known
(ADR-0041).

## Decision

- While a window is fullscreen, Print (and Super+Shift+5) takes the whole screen at once, without
  the choices. The rule is "something has the whole screen", not "a game": videos get the same.
- Selections and recording are still a Super+Shift+3/4 or a leave-fullscreen away; the
  choices come back as soon as nothing is fullscreen.

- The screenshot keys (Print, Super+Shift+3, 4 and 5) join the keys the compositor keeps over a
  fullscreen window (`works_in_fullscreen`, ADR-0041); until then they went to the game, so
  the rule above never ran.

- The picture is drawn as the screen is: the fullscreen window on top goes above the layer
  surfaces, so the bar isn't in a shot of a game. It was, since the picture only drew the layers
  and then the windows.

## Consequences

- A recording already going is still stopped by the key, as before.
