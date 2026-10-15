# 0031. Three-finger drag, on by default

Date: 2026-10-15

## Status

Accepted

## Context

On the laptop, dragging (a window by its title bar, a selection, a file) meant pressing the touchpad
down and moving with another finger, or tap-and-drag. macOS offers three-finger drag under
Accessibility: three fingers moving hold the button and move the pointer, and lifting lets go after
a moment, so a long drag can be made in strokes. The user asked for it, on by default.

libinput recognises three-finger swipes and reports them as gestures, which the compositor dropped;
no client got them (pointer-gestures isn't offered yet), so taking them costs nothing today.
libinput itself has no three-finger drag (it's been discussed upstream for years), so it's done in
the compositor.

## Decision

- On the hardware, a three-finger swipe presses the left button where the pointer is and moves the
  pointer with the fingers, as the touchpad's own motion does (libinput's swipe deltas are
  accelerated the same way). Clients see an ordinary press, motion and release, so every drag works:
  moving and resizing windows, selecting text, dragging files.
- Lifting starts a 500 ms grace; three fingers back down within it carry the drag on without a new
  press. When it runs out, the button is released. A click during the grace drops first.
- Other swipes (two, four fingers) are still ignored. `[touchpad] three_finger_drag` (on by
  default) turns it off; it's read live, so Super+Shift+R applies a change.
- `compositor/src/drag.rs` holds it; the state gains an event-loop handle for the grace timer.

## Consequences

- Dragging on the touchpad is one gesture, as on a Mac with the setting on.
- Three-finger swipes can't be given other meanings (switching workspaces, say) while this is on,
  and won't reach clients if pointer-gestures is added.
- A drag stays held for half a second after lifting, so a drop lands a moment after the fingers
  leave, as on macOS.
- Nested (in a window), the parent desktop keeps its gestures, so it only works on the hardware.
