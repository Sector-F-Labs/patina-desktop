# 0132. Night Light can stand down while a fullscreen window has the keyboard

Date: 2026-11-10

## Status

Accepted. The user asked for the fullscreen detection to be used so Night Light isn't applied while a
game is open.

## Context

Night Light warms the whole screen through the gamma ramp, which a game or a film's colours suffer
from. The compositor already knows when a fullscreen window has the keyboard
(`State::fullscreen_on_top`, ADR-0129), which is what the games and videos that matter are.

## Decision

- **`[night_light] pause_in_fullscreen`**, false by default, so nothing changes unasked. When true,
  Night Light counts as off while a fullscreen window has the keyboard, and fades back (the quick
  one-second fade, not the evening's) when it leaves or loses focus.
- **It is another way of being held off**, beside an app's inhibitor (ADR-0039): `NightLight::active`
  is wanted, not inhibited and not paused. `night_light_update`, which runs each second and on every
  request, hands it whether a fullscreen window is on top, so no new signal is needed.
- **The status says so**: the bus's inhibitors list gains "A fullscreen window" while it's paused, so
  Settings' "Held off by: …" line and `patina-night-light` show why it's off.
- **Settings' Night Light page** has "Off while a game or video is fullscreen", enabled when Night
  Light isn't off.
