# 0004. The toolbox shows only what the chosen tool needs; crop and text join it

Date: 2026-11-19

## Status

Accepted. The user asked for the colour picker to be the one the skin builder uses, under the
tools and shown only for a tool that has a colour, with the size and shape likewise; then, with
that much room, for the tools in five columns, a crop tool and text on the picture. This
supersedes the spectrum strip of [0003](0003-draw-is-for-marking-up-pictures.md).

## Context

The spectrum strip took the bottom of the window whichever tool was chosen, and so did the size
slider and the shape choice, though a pencil has no width and a fill has neither width nor shape.
The skin builder's picker is egui's own (`color_picker_color32`): a field of saturation and
value, a hue slider and the numbers.

## Decision

- **The left panel is the tools, then what that tool uses**, in this order: its colour, its size,
  its shape. Each is absent for a tool that has no use for it (`Tool::uses_colour`, `uses_size`,
  `has_fill`): the picker tool shows nothing, the pencil only a colour, the fill only a colour.
  The panel keeps one width, so it doesn't jump when the tools change, and scrolls when the
  window is short. The spectrum and the bottom panel go.
- **The picker is egui's inline one**, opaque only, 180 px wide. It edits one colour: the
  foreground for most tools; the background for the eraser, which paints it; and for a shape set
  to Both, Outline or Fill, chosen by two tabs above it. egui's numbers are bytes or fractions
  only, so a **hex field** under the picker shows the colour as `#RRGGBB` and takes `#RRGGBB` or
  `RGB`, as soon as what's typed is a colour (`colour.rs`). **A Filled shape is now the chosen colour
  all through** (it was the background colour, as in Paint, which is not what anyone choosing a
  colour for a rectangle expects); only Both has two.
- **Tools are five to a row**, now twelve: rectangle, ellipse, arrow, line, text; highlighter,
  brush, pencil, eraser, fill; picker, crop.
- **Crop** (C): drag a rectangle on the picture, the rest dimmed, with a rule-of-thirds grid and
  its size in the panel. It has eight handles (corners and the middles of the sides) to pull,
  and dragging inside carries it, both kept inside the picture (`crop.rs`). Enter or the Crop
  button (or Image, Crop to Selection) keeps it, as one undo step, and Esc or Clear lets it go.
  Choosing another tool drops it.
- **Crop's shape** is chosen in the panel: Free, Original (the picture's own), 1:1, 4:3, 3:2,
  16:9 or a Custom W:H, with a Portrait toggle that turns a ratio on its side. Choosing one fits
  the largest such rectangle into what's selected (or into the middle of the picture, with room
  round it). Then a new drag and every handle hold it: a corner keeps the opposite corner where
  it is, a side moves and the other way follows about the middle, and where the picture's edge is
  met the shape is kept by giving way on the other side.
- **Text** (T): click where it goes, type in a small box over the picture, Enter writes it
  (Shift+Enter starts a line, Esc drops it); the picture shows the text as it's typed. It is
  rasterised anti-aliased with `ab_glyph` in the Ubuntu font egui ships (`text.rs`), drawn twice
  a fraction of a pixel apart to thicken the light weight, in the foreground colour, its height
  five times the size setting. Choosing another tool or clicking elsewhere writes what's typed.

## Consequences

- The picker takes more room than the strip did, and the right button on the canvas still draws
  the background colour, which a tool that shows only the foreground gives no sight of.
- Text is placed and then fixed: it can't be moved or edited afterwards, only undone. One font,
  one weight, no bold or italics.
- Crop has no numbers to type for its position or size, and no nudging with the keys.

## Revisit when

- Text needs moving, a choice of font or weight, or a background behind it.
- Crop needs typed numbers or nudging with the arrow keys.
