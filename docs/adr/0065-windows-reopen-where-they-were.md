# 0065. Windows reopen where they were, by app id, decided by the tiler

Date: 2026-10-18

## Status

Accepted. Asked for by the user. It was first written into the compositor; the user asked for it
in the tiler, where decisions about where windows go belong
([ADR-0053](0053-tiling-decided-by-patina-tiler.md)).

## Context

Under Wayland a window doesn't know where it is and can't ask to be anywhere, so an app can't
reopen its window where it was the way X11, Windows and macOS apps do. Only the compositor's side
can. Patina cascaded every new window from the top left of the work area
([ADR-0007](0007-floating-first.md)).

The standard answer is session management (`xdg-session-management`): the app is given an id for
each window to hand back next time. Few apps speak it yet. The app id is what every window has.

Apps still choose their own size, often the last one they had. On a smaller screen than the one
they last ran on, that can be bigger than the work area, with the window sticking out past the
edge of the screen.

## Decision

- **The compositor reports, the tiler decides.** The tiling bus
  ([ADR-0049](0049-tiling-by-dragging-decided-over-d-bus.md)) gains
  `WindowOpened(id, app_id, placement, x, y, w, h)`, sent on a Wayland window's first buffer,
  when its app id and size are known (not for a window that asked for fullscreen first);
  `WindowClosed(id, app_id, x, y, w, h)`, with where it floated, or did before it was placed or
  went fullscreen; and `Move(id, x, y)` for a floating window, since `Place` tiles.
- **patina-tiler remembers where each app's window was, by app id**, in
  `~/.local/state/patina-tiler/windows.toml`: from `WindowClosed`, and from a drop where the
  window floats, so a session ending before the window closes still keeps it.
- **A new window opens there if no other window of its app is open** and it isn't maximised, or
  filling the work area if that window closed filled. A
  second window of the same app stays where the compositor cascaded it, rather than opening on
  top of the first.
- **A window that sticks out of the work area is moved back inside it, and one too big for it
  fills it** (as dragging to the top does) instead of hanging off the screen. `policy::opening`
  decides all three, tested.
- Only Wayland windows: X11 windows still go where they ask, or cascade.
- **The tiler exits when the compositor it started with leaves the bus**, or another takes its
  name, as its man page always said. Trying this showed tilers outliving their compositors: the
  oldest held the Tiler name for good, so every newer tiler, with this, queued behind it unheard.

## Consequences

- Apps reopen where they were across restarts of the app, the tiler and the compositor, and
  changing how is a tiler restart, not a desktop restart.
- A window shows for a frame or two at its cascaded spot before the tiler's `Move` arrives. The
  compositor could hold the window back until the tiler answers if that shows.
- A window too big for the screen draws at its own size before it fills.
- An app whose windows all share one app id (a browser with several windows) reopens only its
  first window where the last one closed.
- Another tiler gets the same signals and can do it differently, or not at all.
- Session management, when apps speak it, can replace the app id for those apps.
