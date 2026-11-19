# 0012. Quick Look's button is Edit for a picture, and opens it in Draw

Date: 2026-11-19

## Status

Accepted. The user asked for the Open button in Quick Look to become an Edit button that opens
the picture in Draw, so Draw is the editor a picture gets from Files. The type list and the way
Draw is found are my design.

## Context

Quick Look (ADR-0008) had one button, Open, which hands the file to `xdg-open`: for a picture,
the viewer (Loupe and its kin in the Patina mimeapps list). Pressing Space on a picture is
usually the step before changing it, and Draw, the Patina desktop's paint program, edits pictures
and takes the file as its argument. The mimeapps lists name a viewer first for pictures, so
"the default app" can't be the editor, as the text editor is for a script's Edit (ADR-0011).

## Decision

- **For a picture Draw saves (PNG, JPEG, BMP, GIF, WebP), Quick Look's button says Edit** in
  place of Open and opens the file in Draw, then closes Quick Look as Open does. Other files
  keep Open. Enter still opens in the viewer.
- **Draw is found by its desktop entry**, `com.sectorflabs.Draw.desktop`, its `Exec` filled in
  for the file, else `draw` from the path (`editor::picture_command`). The mimeapps lists aren't
  read: Draw isn't, and shouldn't be, the default for pictures.
- **SVG isn't included**: Draw is a bitmap editor and would rasterise it.

## Consequences

- Draw saves over the file it was given when asked to (Ctrl+S), as any editor does; Files takes
  no copy first.
- If Draw isn't installed the button says so in the status message rather than falling back to
  the viewer.

## Revisit when

- The context menu should offer Edit for pictures as well (it does for scripts).
- Draw edits more types, or a different image editor is chosen: it becomes a mimeapps
  association rather than a constant.
