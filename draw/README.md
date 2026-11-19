# Draw

The Unix MS Paint: a small bitmap paint program for sketching, annotating and quick fixes. A
[Patina](https://github.com/Sector-F-Labs/patina) (egui) app, so it takes Patina's skins and
follows the desktop's light/dark setting.

![Draw](docs/screenshot-dark-orange.png)

Made for marking up a picture: take a screenshot, draw a rectangle, an arrow or a circle on it,
highlight something, and send it on. Files' Quick Look opens a picture here with its Edit button.
Rectangle, ellipse, arrow, line, text, highlighter, brush, pencil, eraser, fill, colour picker and
crop (with handles, and a shape to hold it to), as symbols in the toolbox, which shows a tool's colour (the skin builder's picker), size and
shape only when it has any; undo and redo; zoom; flip, rotate, invert and canvas
size; open PNG, JPEG, BMP, GIF and WebP, and save PNG, JPEG and BMP. Save As starts with the
same name and ` (edit)`, so the original stays. `draw picture.png` opens a file.

## Keys

| Key | |
|---|---|
| R O A L T | Rectangle, Ellipse (O), Arrow, Line, Text |
| H B P E F K C | Highlighter, Brush, Pencil, Eraser, Fill, Picker (K), Crop |
| Enter / Esc | Keep the crop / drop it; write the text / drop it (Shift+Enter: new line) |
| X | Swap the foreground and background colours |
| Shift while dragging | Lines at 45° steps; squares and circles |
| Esc | Cancel the stroke in progress |
| Ctrl+N / O / S / Shift+S | New, Open, Save, Save As |
| Ctrl+Z / Ctrl+Shift+Z | Undo, Redo |
| Ctrl+C | Copy the picture to the clipboard |
| Ctrl+R / Ctrl+I | Rotate clockwise, invert colours |
| Ctrl+wheel, Ctrl+ +/− | Zoom; Ctrl+0 actual size, Ctrl+9 fit |

The right mouse button draws with the background colour and swaps a shape's outline and fill.
The Picker sets the foreground on the left button and the background on the right.

## Build

```bash
make            # list the targets
make run        # run it
make check      # tests + clippy -D warnings
make install    # per user, under ~/.local (PREFIX=... to change)
```

Settings are in `~/.config/draw/config.toml`; your own skins go in `~/.config/draw/skins/`.

## Not yet

Selections, text, layers, antialiasing and pasting an image in. See
[ADR-0002](docs/adr/0002-cpu-bitmap-and-snapshot-undo.md).

## Licence

BSD-3-Clause.
