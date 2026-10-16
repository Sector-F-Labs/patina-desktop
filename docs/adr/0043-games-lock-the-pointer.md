# 0043. Games lock or confine the pointer, and read the mouse's motion as such

Date: 2026-10-17

## Status

Accepted

## Context

Half-Life under Xwayland (ADR-0040) aimed the wrong way, or jumped, depending on which side of the
window's centre the pointer was. It looked like natural scrolling, but that setting only touches
scroll events. Mouse-look in X11 games works by warping the pointer to the window's centre each
frame and reading how far it moved since. Xwayland can only hand that on when the compositor offers
both relative-pointer (motion as distances, whatever the pointer's position) and pointer-constraints
(a client locking the pointer in place or confining it to its window). The compositor offered
neither, so Xwayland reported the jump from the centre back to the real pointer as the motion.
Wayland games need the same two protocols directly.

## Decision

- The compositor offers `zwp_relative_pointer_manager_v1` and `zwp_pointer_constraints_v1`
  (Smithay's), in `constraints.rs`.
- libinput motion goes through `State::pointer_moved`. It sends the motion as relative motion
  (accelerated and not) to the client under the pointer. A locked pointer then stays put. A confined
  pointer moves only while it stays on that surface and inside its region. Otherwise the pointer
  moves as before, kept on screen.
- A constraint is activated when the pointer is over its surface (and inside its region), on
  creation or after a move. Smithay deactivates it when the pointer leaves. A locked client's
  cursor position hint moves the pointer there, so it's in the same place when the lock ends.
- Three-finger drag (ADR-0031) still moves the pointer directly, without relative motion.

## Consequences

- Mouse-look works in X11 and Wayland games, on the hardware.
- Nested (winit or the sctk fallback), the parent desktop only gives absolute positions, so games
  still misbehave there. Asking the parent for its own locked pointer would fix that, and is the
  way to test games without logging in again.
- A constraint holds while the pointer is over its surface, whether or not that window has the
  keyboard, as in Smithay's example compositor. A background window could hold the pointer if it
  asked; GNOME requires focus. Revisit if that bites.
- Super and the compositor's other keys still work while the pointer is locked. There's no key of
  the compositor's own to break a lock; the game releases it in its menus.
