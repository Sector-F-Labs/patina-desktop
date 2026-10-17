# 0047. The screen turns off after a while without input, unless something is playing

Date: 2026-10-17

## Status

Accepted

## Context

GNOME, and most desktops, turn the screen off after a few minutes without input (GNOME's "Screen
Blank", five minutes by default) and turn it back on at a key or a movement. Patina's compositor
never did, so a laptop left alone kept its panel lit. The user asked for it, set in Settings.

What was weighed:

- **Turning the screen off.** On the hardware, Smithay's `DrmCompositor::clear()` sets the CRTC's
  DPMS to off and clears its planes; the next frame queued turns it back on. Nested, the parent
  desktop looks after its own screen, as with Night Light (ADR-0039). Drawing black nested was
  ruled out: it saves nothing and isn't what a nested desktop should do.
- **What counts as input.** Keys, pointer movement, clicks, scrolling and touchpad gestures, as the
  compositor sees them. Gamepads it never sees, which matters for games.
- **What holds the screen on.** Video players and browsers use the Wayland idle-inhibit protocol,
  which Smithay has. Steam holds `org.freedesktop.ScreenSaver`'s Inhibit on the session bus while a
  game runs, as do SDL games and video players under X; that's how a game played with a controller
  keeps the screen lit. On GNOME that service comes from the desktop; in a Patina session nothing
  served it. A fullscreen window holding it on by itself was considered and left out, as GNOME
  doesn't.
- **ext-idle-notify,** for idle daemons such as swayidle, isn't needed for this and was left for
  later.
- **The input that wakes the screen** is passed on as usual, not swallowed, as GNOME does it.
  Moving the pointer is the usual way to wake it.
- **Locking.** GNOME locks when the screen turns off by default. Here it's an option, off by
  default for now.

## Decision

- **The setting:** `[screen] off_after` in the compositor's config, seconds without input before
  the screen turns off; 300 by default, 0 for never. `[lock] on_screen_off` locks it as it turns
  off. Both reload within a second of a save (ADR-0039), so Settings only writes them.
- **The timer** (`compositor/src/idle.rs`): every input event counts as activity. Once a second,
  if `off_after` has passed since the last and nothing holds the screen on, it turns off. Something
  holding it on counts as activity, so when it lets go the full wait starts again.
- **On the hardware,** `UdevData::power_off` clears the DRM compositor and nothing is drawn until
  `power_on`, at the next input, which also sets Night Light's gamma again. While it's off, a lock
  counts as on screen, so going to sleep isn't held back waiting for it to be drawn. Coming back
  from another console counts as input. Nested, the timer runs and logs, and nothing else happens.
- **Holding it on:** the idle-inhibit protocol, for any live surface that asks; and
  `org.freedesktop.ScreenSaver` on the session bus, at `/org/freedesktop/ScreenSaver` and
  `/ScreenSaver`: `Inhibit(app, reason) → cookie`, `UnInhibit(cookie)`, `SimulateUserActivity()`
  and `GetActive()` (whether the screen is off). What's held is dropped when its holder leaves the
  bus. Where the name is taken (nested in GNOME), the parent desktop's applies.
- **Settings:** the Power page gains a Screen section: "Turn the screen off after" with GNOME's
  choices (Never, 1 to 5, 8, 10, 12 and 15 minutes, and any other time set in the file) and "Lock
  the screen when it turns off".

## Consequences

- An idle laptop's panel goes dark after five minutes, and comes back at a touch.
- A game played only with a controller, and not through Steam or SDL's inhibit, will have its
  screen turned off. If that bites, a focused fullscreen window could hold it on.
- Clients get no frame callbacks while the screen is off, so they stop drawing, which saves power;
  ones that hold it on are never in that state.
- There's no dimming before it goes off, as GNOME has, and nothing tells the user what is holding
  the screen on; both could come later, the second on Settings' Power page.
- It can't be seen working in the VM, which is nested: only the log shows the timer. The DPMS
  call needs real hardware to check.
