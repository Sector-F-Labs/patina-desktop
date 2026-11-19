# 0005. The wheel zooms about the pointer; Shift and Alt, a touchpad, and drags pan

Date: 2026-11-19

## Status

Accepted. The user asked for sensible zoom controls, the mouse wheel to zoom and Shift to pan, as
is usual. What is usual differs by program (Photoshop and GIMP scroll with the wheel and zoom
with Alt or Ctrl; map and viewer programs zoom with it), so the choice below is mine.

## Context

Draw zoomed only in fixed steps (12.5% to 3200%), by Ctrl and the wheel or the keys, and always
about the top left, so what you were looking at moved away; the plain wheel scrolled up and down.
A picture being marked up is looked at closely and often: the wheel is the quickest way in.

## Decision

- **The wheel zooms, about the pointer**: the point of the picture under it stays under it
  (`view::zoom_about`). A notch is 1.2 times, any zoom from 5% to 6400%, not only the steps.
  Ctrl and the wheel zooms too, for the muscle memory of those who have it, and so does a pinch.
- **Shift and the wheel pan sideways, Alt and the wheel up and down.** A touchpad's two-finger
  swipe pans (it is told from a wheel by arriving in pixels, not notches), and with Ctrl it zooms.
- **The middle button, or Space and the left one, drags the picture about**, with a hand for a
  cursor, without drawing.
- **The keys and the menu still step** through 12.5% … 3200% (from wherever the wheel left it),
  about the middle of the canvas; Ctrl+0 is actual size and Ctrl+9 fits.
- **Zoomed out the picture is smoothed, zoomed in each pixel is sharp**: nearest-neighbour
  scattered a screenshot into noise at 50%.

## Consequences

- The plain wheel no longer scrolls: up and down is Alt and the wheel, the scroll bars, a
  touchpad or a middle-button drag. Someone who wants the usual scroll-and-Ctrl-zoom has to learn
  this one.
- The scroll area keeps its bars but not the wheel; `view.rs` holds the arithmetic and `look` in
  `app.rs` applies it.

## Revisit when

- A setting for which of the two the plain wheel does is asked for.
- Zooming on a pointer-less keyboard (the keys zoom about the middle) needs the selection or the
  cursor as the anchor instead.
