# 0034. Screenshots with the Mac's keys, taken by the compositor

Date: 2026-10-15

## Status

Accepted

## Context

There was no way to take a screenshot on the desktop. Its keys already follow the Mac where there's
one to follow (Super+Q quits an app, Ctrl+Super+Q locks), and macOS has Cmd+Shift+3 for the whole
screen and Cmd+Shift+4 for a part of it (with Space, a window). Keyboards also have a Print Screen
key, which should do the obvious thing.

grim and similar tools ask the compositor for the screen over wlr-screencopy, which Patina doesn't
offer; a tool would also need the focused window's place, which only the compositor knows. The
compositor already draws everything with a renderer that can draw into an image of its own and read
it back (GLES on the hardware and in a window, pixman as the fallback).

## Decision

- **Super+Shift+3** and **Print** take the whole desktop; **Super+Shift+4** takes the focused
  window. They're matched on the key's base symbol, so the digit is the same with Shift held and on
  any layout. They're the compositor's own keys, not `[bindings]`, and do nothing while locked.
- The compositor draws what it would show into an offscreen image of the output's size and scale,
  with the same renderer: the desktop without the pointer, as macOS takes it, over the usual
  background; or the window alone, cut to its edges (not the shadow a client draws around them),
  on transparency.
- A thread saves it as a PNG in the Pictures folder's `Screenshots` (`~/Pictures/Screenshots`),
  named as macOS names them: `Screenshot 2026-10-15 at 21.04.33.png`, with ` (2)` and on when two
  come in one second.
- Once it's taken, a white flash fades out over what was taken (the screen, or the window), in
  about a third of a second, drawn after the picture so it isn't in it.
- For now there's no selection (Cmd+Shift+4's drag), no picking a window with the pointer, no
  clipboard and no sound; Super+Shift+4 takes the focused window outright.

## Consequences

- Screenshots work in every backend without another program or protocol.
- The flash shows it was taken; the file's name is in the compositor's log.
- Other tools still can't capture the screen (no wlr-screencopy); offering it is separate.
