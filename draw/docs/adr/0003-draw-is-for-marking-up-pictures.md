# 0003. Draw is for marking up a picture: annotation tools first, symbols, Save As, a spectrum

Date: 2026-11-19

## Status

Accepted; the palette decision is superseded by [0004](0004-the-toolbox-shows-what-the-tool-needs.md). The user said what Draw is mostly for: someone takes a screenshot, draws a rectangle,
an arrow or a circle on it, or highlights something, and sends it on; and Files' Quick Look now
opens pictures in it (Files ADR-0012). The tools, their order, the highlighter's blend and the
spectrum's layout are my design.

## Context

Draw began as a joke, the Unix MS Paint: eight tools named in words, Paint's 28-swatch palette,
and a Save button. Marking up a screenshot needs the tools Paint lacks (an arrow, a highlighter),
leaves the original alone (a new file, not a save over it), and wants any colour in a click, not
the nearest of 28.

## Decision

- **Two tools: Arrow and Highlighter.** The arrow is a line with a filled head whose tip is where
  the pointer is let go, sized with the pen (Shift keeps it to 45° steps, as for a line). The
  highlighter is a broad stroke (three times the pen size, at least 8 px) that multiplies each
  channel by the colour, as ink on paper does: white under it takes the colour and black text stays
  black. A stroke is redrawn from the untouched picture each time it grows, so going back over
  itself doesn't darken it.
- **The toolbox order is the markup order**, two to a row: rectangle and ellipse, arrow and line,
  highlighter and brush, pencil and eraser, fill and picker. Rectangle is the tool Draw starts
  with and red the colour, so a picture opened to mark can be marked at once.
- **The tools are symbols**, painted with shapes (`icons.rs`) in the skin's colours, with the name
  and key in the tooltip. A bitmap or font glyph would not follow the skins.
- **Save As sits beside Save** in the header. It starts in the picture's folder with the same
  name and ` (edit)` before the extension (`photo (edit).png`), so the original stays; a name
  already ending so is kept, and a type Draw can't write (a GIF, a WebP) is offered as a PNG.
  Save still writes over the file, as in any editor.
- **The palette is a spectrum**, in the same place and height (`spectrum.rs`): hue along it, a
  colour going from white through its full strength to black down it, flat bands at the top and
  bottom for exact white and black, and a column of greys at the end. Left button sets the
  foreground, right the background, each marked where it was taken from. The two swatches stay,
  and open an editor for an exact colour.

## Consequences

- Any colour is reachable, but not by name: there's no row of favourites to return to. A picked
  or typed colour that isn't on the spectrum shows no mark.
- The highlighter and arrow are as hard-edged as every tool here: no anti-aliasing, which shows
  on a slanted arrow at small sizes.
- Redrawing a highlighter stroke copies the picture on every move, as a shape does; fine at
  screenshot sizes, a cost at very large pictures (ADR-0002).

## Revisit when

- Text on a picture (a label next to the arrow) is asked for: the next annotation tool.
- Anti-aliased strokes, or favourites beside the spectrum, are asked for.
