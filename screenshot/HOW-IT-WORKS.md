# How screenshots and recordings fit into the desktop

Press Print and the screen is taken, or a small pill offers a window, a selection or a
recording. This page explains how that is split between the compositor and a small program, and
why the program does so little. The README next to it covers using and building it; the
decisions are in ADR-0034, ADR-0091, ADR-0092 and ADR-0102.

## The idea in one paragraph

On Wayland a normal client can't ask for a picture of the desktop, and can't see what is on
another client's window. On Patina the compositor is the one process that draws everything, so
it takes every picture itself, with the renderer it already has. `patina-screenshot` is only a
pill that asks "which one?" and prints the answer. It never sees a pixel. The two meet over a
child process's stdout: one line, once.

## The picture

<img src="images/architecture.svg" alt="The compositor sees the Print key. It either takes the screen at once, or starts patina-screenshot, which prints a choice. Screen and Window go to take_screenshot, which draws offscreen, flashes and saves a PNG on a thread. Selection goes through select.rs first. Recording feeds frames to ffmpeg. Separately, OBS or a browser asks the portal, which asks patina-screenshot --share for the choice and capture.rs for frames.">

Three things happen in the compositor box, and none needs a protocol: seeing the key, drawing
the picture, and writing it out. The green boxes are the only separate program.

## The sequence, in time

<img src="images/sequence.svg" alt="One screenshot in time: press Print, the compositor keeps the focused window and starts patina-screenshot, you choose Screen, it prints the choice and exits, the compositor draws offscreen, hands the picture to a thread, and flashes the screen.">

### Why it works in this order

- **The window is kept before the pill opens.** The pill takes the keyboard, so by the time you
  choose Window the compositor no longer has a focused window to ask. It remembers the one that
  was focused at the key press (`shots.window`), and takes nothing if it closed meanwhile.
- **Drawing waits for the program to exit.** The thread reads the choice until the pipe closes
  and then waits for the child, so the pill's surface is gone before anything is drawn. The
  compositor also leaves layers in the `patina-screenshot` namespace out of every picture, in
  case one is still mapped.
- **The flash comes after the picture.** It is drawn on later frames, so it shows that
  something was taken without being in it.

## The Print key over a game

With nothing fullscreen, Print opens the pill. While a fullscreen window is *on top* (alive,
focused, and the screen not locked: `fullscreen_on_top()`), it takes the whole screen at once
instead, since the pill would take the keyboard from the game (ADR-0102). Two details that
followed:

- The screenshot keys are among the few the compositor keeps over a fullscreen window
  (`works_in_fullscreen`); before that they went to the game and the rule never ran.
- The picture is drawn as the screen is, with the fullscreen window above the bar, so a shot
  of a game doesn't have the bar in it.

The rule is "on top", not "registered as fullscreen". A window can stay marked fullscreen while
something else has the focus, and Print then has to behave as it does on a normal desktop.

## What the compositor does that a client can't

| Job | How it's done here | Why a normal client can't |
|-----|--------------------|---------------------------|
| Draw the desktop to an image | the same renderer it draws the screen with (GLES, or pixman as a fallback), into an offscreen target (`screenshot.rs`) | no protocol hands a client other windows' pixels; Patina offers wlr-screencopy to none |
| Take one window, to its edges | it knows the window's rectangle and its frame, and leaves out the shadow a client draws around itself | a client doesn't know where another's window is |
| Know which window was focused | it kept it when the key went down | a client has the keyboard only once the key is already gone |
| Leave the pointer out, and the pill, and the flash | it decides what goes in the scene | everything else shows up as it is on screen |
| Drag out a selection | `select.rs`: solid-colour elements for the dim and the edge, and the pointer and clicks kept from every window meanwhile | a client can't take the pointer from other windows |
| Record | `capture/record.rs` draws the scene offscreen, up to 30 times a second while it changes, and pipes the pixels to `ffmpeg` | no access to the pixels |

## Selections and recording

A selection is not the pill's job either. After Selection or Record Selection the pill exits,
and the compositor starts `select.rs`: the screen dims under a crosshair, the dragged part
shows through with a one-pixel white edge, and letting go either takes it
(`take_screenshot` with an area, which crops the full picture) or starts recording it.
Escape, another button, or a click without a drag takes nothing.

Recording draws the same scene each time the screen changes (the indicator left out), cuts
out the selection, and writes the pixels to `ffmpeg`'s stdin. `ffmpeg` stamps each frame with
the time it arrives and writes a steady 30 a second, repeating the last while nothing moves. A
second `patina-screenshot --recording` shows a red dot, the time and Stop; Stop prints `stop`,
and so does the Print key, which stops a running recording before it opens anything.

## What `patina-screenshot` doesn't know

It has three modes, and none takes or records anything:

1. **The choices.** Started by the compositor on Print. It prints `screen`, `window`,
   `selection`, `record-screen` or `record-selection`, or nothing if closed, and exits.
   `--no-window` greys Window out.
2. **`--recording`.** Started with a recording, until the compositor closes its stdin.
3. **`--share`.** The chooser xdg-desktop-portal-wlr runs when OBS or a browser asks to share
   the screen (`chooser_type=dmenu`, set by `make install`). It reads `Monitor: …` and
   `Window: …` lines on stdin and prints the one picked; nothing on Escape, which the portal
   takes as a no.

It has no Wayland knowledge beyond its own surface, so it could be built and looked at on its
own, as PNGs drawn in each skin, before the compositor side existed.

## The bottom of the picture: sharing the screen

This is a different path from the Print key. An app asks the portal, which asks
`patina-screenshot --share` which screen or window it may see, and then asks the compositor's
`capture.rs` (ext-image-copy-capture, and ext-foreign-toplevel-list for the window list) for
frames, which it streams over PipeWire. A frame waits until what it shows has changed, then
the scene is drawn offscreen like any screenshot and copied through memory (no dmabufs yet).
It shares the drawing with the Print key, and nothing else.

## The files

```
 patina-desktop/
 ├── compositor/src/
 │   ├── input.rs              the screenshot keys; works_in_fullscreen
 │   ├── keys.rs               [keys] screenshot, screenshot_window, screenshot_menu
 │   ├── screenshot.rs         take_screenshot: draw offscreen, crop, flash, PNG thread
 │   ├── screenshot/menu.rs    starts the pill, reads its choice, acts on it
 │   ├── screenshot/select.rs  the dim, the crosshair and the dragged rectangle
 │   ├── capture/record.rs     frames to ffmpeg, the MP4
 │   └── capture.rs            ext-image-copy-capture for other programs
 └── screenshot/src/
     ├── main.rs               picks the mode
     ├── hud.rs                the pill with the choices
     ├── indicator.rs          the recording's pill
     └── share.rs              the portal's chooser
```

Screenshots are saved in `~/Pictures/Screenshots` and recordings in `~/Videos/Screen
Recordings`, named as macOS names them (`Screenshot 2026-11-03 at 21.04.33.png`, with ` (2)`
and on when two come in a second).

The diagrams in `images/` are hand-drawn SVG, and follow the viewer's light or dark theme.

## Limits, as they stand

- One output: a selection is one output's, and isn't frozen while it's made.
- Over a fullscreen window Print takes the screen only; selections and recordings need a
  Super+Shift key or leaving fullscreen first.
- Sharing copies through memory, not dmabufs.
- Recording needs `ffmpeg` on the path.
