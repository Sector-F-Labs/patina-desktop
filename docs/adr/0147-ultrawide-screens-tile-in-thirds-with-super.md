# 0147. Ultrawide screens tile in thirds with Super held

Date: 2026-11-16

## Status

Accepted. Builds on [0146](0146-dragging-with-super-tiles-to-the-half-the-pointer-is-in.md).

## Context

On a 21:9 or 32:9 screen a half is very wide, and the useful layouts are columns: a third for a
chat or a terminal, two thirds for the work beside it. Dragging with Super held (ADR-0146) only
offered halves.

## Decision

- **Only on ultrawide screens**, those whose width is at least twice their height
  (`policy::ULTRAWIDE`; 21:9 is 2.33, 16:9 is 1.78, 16:10 is 1.6). Other screens keep halves.
- **With Super held, the screen's width is in three zones:** the left third tiles to the left
  third, the right third to the right third, and the middle third to two thirds, the left two
  when the pointer is left of the middle and the right two when it is right of it. The top edge
  still fills. The preview shows it as the pointer moves.
- **Four new tiles**, `LeftThird`, `RightThird`, `LeftTwo` and `RightTwo`. A third is the area
  less two gaps, divided by three, and the two thirds are what a third and a gap leave, so a
  third and the two thirds beside it fill the area exactly.
- **Thirds stand alone.** They take no part in the split of the two halves' columns (ADR-0054):
  dragging the line beside one moves nothing else, and a third beside a half doesn't change what
  the half keeps. Super+Left and Right restore a third from the other side, and tile to a half
  from the same one.

## Consequences

- No tile for the middle third; two thirds leave a third free on a side, and the other drop
  zones are the thirds either side.
- Without Super, dragging to the edge still gives halves on every screen.
- The aspect ratio is read from the screen the tiler was told of; it is not set by the user.
