# 0094. The touchscreen's touches go to apps as wl_touch, without pointer emulation

Date: 2026-10-26

## Status

Accepted: tested on nifl.

## Context

nifl, the laptop, has a touchscreen (`ELAN901C:00 04F3:2C29`, beside its touchpad). The
compositor ignored libinput's touch events, and its seat had no touch capability, so apps were
never told there was one. On GNOME, apps that handle touch (GTK, Qt, Firefox, Chromium, and X11
apps through Xwayland) get it as wl_touch, and the rest don't answer to it.

There are three parts to doing touch fully:

1. Touches into the seat, to the surface under them.
2. Touches turned into pointer events where nothing reads touch: the compositor's frames,
   title-bar drags, the line between tiled windows and apps without touch.
3. wl_touch in patina-shell's host, so the bar, menus and launcher answer to it.

## Decision

- **Only the first part for now.** The seat has touch whether or not there's a touchscreen.
  libinput's touches go to the surface under them, found as the pointer's is (`surface_under`),
  mapped over the whole screen: there's one output, never rotated.
- **A touch going down focuses as a click does:** the window it lands on is focused and raised;
  a layer surface takes the keyboard only if it asks for it; locked, it goes to the locker and
  changes nothing else.
- **Each finger's motion goes to the surface it went down on**, through Smithay's own touch grab.
- **Nothing is turned into pointer events**, and the screenshot selection, frames and tiling
  lines don't see touch.

## Consequences

- Apps that handle touch on GNOME handle it here: scrolling, pinching and tapping in browsers
  and GTK apps.
- Apps without touch, the compositor's frames and the Patina pieces don't answer to it. Windows
  can't be moved by finger.
- Pointer emulation (part two) and patina-shell's wl_touch (part three) come later, in ADRs of
  their own. A pen would need tablet-v2, which is still a stub.
- A second output or a rotated screen would need touches mapped to the built-in panel's output,
  not the whole screen.
