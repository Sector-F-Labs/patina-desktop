# 0085. Wayland windows that ask for a frame get Patina's, through xdg-decoration

Date: 2026-10-24

## Status

Accepted. The user asked for xdg-decoration so Wayland windows get frames too, after seeing
pygame (SDL 2, on X11 through Xwayland) framed and asking whether Wayland windows were. This is
the Wayland half ADR-0077 left for later.

## Context

ADR-0077 frames X11 windows that draw none, with patina-ui's `TitleBar` painted by the
compositor. Wayland windows got nothing: there was no xdg-decoration global, so every Wayland app
drew its own frame or none. foot, Qt, mpv, Blender and SDL's games through libdecor all prefer
the server's frame and fall back to a plain one of their own, or a bare window, without it.

In xdg-decoration the compositor announces the global, a client creates a decoration object for
its toplevel and may ask for `client_side` or `server_side`, and the compositor answers in a
configure. winit, so every Patina app, asks for `client_side` when the app opens with
`decorations(false)`, as Patina's frame does, and for `server_side` otherwise. GTK doesn't use
the protocol at all.

## Decision

- **The compositor offers xdg-decoration and gives a window the mode it asks for:** `server_side`
  when it asks for that, says it doesn't mind (`unset_mode`) or has only made the object;
  `client_side` when it asks for that. Patina apps and GTK keep their own frames.
- **A Wayland window whose current (acknowledged) mode is `server_side` is framed** as an X11 one
  is (`frames::wants_frame`): the same title bar, border and edges, the same rules for
  fullscreen. Its title is the toplevel's; close is `xdg_toplevel.close`; an edge starts the
  compositor's xdg resize (`ResizeGrab`).
- **The frame is part of the window's outer rectangle** as in ADR-0077, now on the Wayland paths
  too: a window that floats opens with its frame where the cascade put it and the window below
  (`window_shown`); `retile` gives the window the tile less the title bar; the tiler is sent and
  sends rectangles with the frame (`Opened`, `Closed`, `Windows`, `Move`).

## Consequences

- foot, Qt apps, mpv, Blender and SDL games on Wayland get Patina's title bar, in the bar's skin.
- An app that asks for `server_side` but draws its own title bar anyway gets two; such an app
  is broken under KWin and sway too.
- The positions the tiler keeps for an app (ADR-0065) were the window's own rectangle and are now
  the outer one, so an app that newly takes a frame reopens one title bar higher, once.
- The title bar holds no app menu or actions: an app that wants them draws its own frame.

## Revisit when

- KDE's `org_kde_kwin_server_decoration` is wanted for GTK 3 apps that ask through it.
- Apps want the frame's buttons themed per app, or a window menu on its title bar.
