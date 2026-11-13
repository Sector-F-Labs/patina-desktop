# 0136. Minimising and restoring are animated: the window shrinks and fades to the window list

Date: 2026-11-13

## Status

Accepted. The user asked for it, after the four-finger swipe ([0135](0135-a-four-finger-swipe-minimises-every-window.md))
minimised every window at once with nothing to show where they went.

## Context

A minimised window was taken out of the space and was gone on the next frame
([0084](0084-minimise-to-the-bars-tabs.md)). One window is easy to follow; with the swipe, the
screen is suddenly empty. The compositor had no animations for windows.

Ways to do it:

- **Animate the real window in the space**, moving and scaling it. A window's size is the client's,
  and the space places windows, not scales them; a scale would have to be made up for in input,
  focus and damage. Rejected.
- **Draw a ghost**: unmap the window as before, and draw its surfaces (and its frame) for a short
  while from where it was, scaled and moved, then stop. Smithay's `RescaleRenderElement` and
  `RelocateRenderElement` do that for any render element, and the surface and frame elements take an
  alpha. Chosen.

## Decision

- `minimize` starts a ghost at the window's place; `unminimize` starts one going the other way
  (the window is mapped at once, and drawn as the ghost, not itself, until it arrives). Every route
  there is, the key, the button, the swipe, a tab, Alt+Tab, takes it.
- A ghost takes 280 ms, eased in and out (as the HUD's parts do, [0111](0111-hud-elements-and-hiding.md)).
  It shrinks to a sixth of its size about its top left corner, which is moved so its middle goes to
  the middle of the window list, and fades from opaque to nothing. Coming back is the same backwards.
- **The window list** is the layer surface `patina-hud-windows` of the monitor the window is on
  (`patina-hud` if the HUD isn't split), wherever the HUD puts it, whichever edge. With none, the
  middle of the screen's bottom edge.
- Ghosts are drawn above everything else in the desktop, in `desktop_elements`, on the hardware, in a
  window and nested; screenshots and capture don't take them. Each frame drawn asks for the next
  while one is on its way (`State::tick_ghosts`).

## Consequences

- The ghost is the window's surfaces and its title bar. Its border, rounded corners (ADR-0133's web
  apps) and shadow aren't drawn on the way, so they go at the start and return at the end.
- It goes to the middle of the window list, not to the window's own tab: the compositor doesn't know
  where a tab is, since the HUD draws them.
- A ghost is a snapshot of what the window last drew; a window that was minimised and changes while
  it comes back shows the change when it arrives.
- Every window takes its own ghost, so a swipe with many windows draws many at once; on the CPU
  renderer ([0002](0002-software-rendering.md)) that may be slow.
- There's no setting to turn it off yet. I haven't watched it on a screen, so the duration, the
  size it shrinks to and how soon the fade starts are first guesses.
