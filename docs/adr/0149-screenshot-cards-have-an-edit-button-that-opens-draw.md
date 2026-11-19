# 0149. A screenshot's card has an Edit button that opens Draw, and the card follows the file

Date: 2026-11-19

## Status

Accepted. The user asked for an Edit button on the cards that announce a screenshot (ADR-0143),
to edit it, click Save, and drag the card out afterwards with the edited version landing in the
drop. Builds on [0143](0143-screenshots-are-notifications-you-can-drag-out-not-clipboard-entries.md)
and Files' ADR-0012, which does the same from Quick Look.

## Context

A file card's drag already offers the screenshot file itself, read when the drop asks for it
(ADR-0143), so a changed file is what lands. Two things stood in the way of editing: no way to
start an edit from the card, and the card's picture, read once, would go on showing the old one.

## Decision

- **A file card for a picture Draw saves (PNG, JPEG, BMP) has an Edit button**, with the card's
  own buttons, under the picture. It runs `draw <file>` detached and leaves the card where it is:
  the card has not done its job until the picture is dragged or dismissed.
- **Save in Draw writes over the screenshot**, so the card's file is the edited one and the drag,
  which reads the path at the drop, hands out the edit. Save As makes a copy beside it
  (`name (edit).png`, Draw's ADR-0003); the card keeps standing for the original.
- **The card follows its file.** The column looks at each card's file's modified time once a
  second and reads the picture again when it changes. The old picture stays until the new one is
  ready, and if reading fails (the file is being written) the old one stays and the next change
  reads again. This holds for any change to the file, not only Draw's.

## Consequences

- Editing a screenshot destroys the original: the first Save replaces it. Undo ends with the
  window; the way to keep the original is Save As, which the card does not follow.
- The column wakes once a second while it has cards, as it already did for the config.
- The Edit button needs Draw installed and on the session's `PATH`; without it nothing happens
  but a line on the daemon's stderr.

## Revisit when

- The card should follow a Save As too (Draw telling the daemon where it saved), or keep the
  original first.
- Another editor than Draw is wanted: the choice would come from the desktop's settings, as
  Files' scripts do (Files ADR-0011).
