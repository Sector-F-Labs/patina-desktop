# 0077. X11 windows that draw no frame get one from the compositor, painted with egui

Date: 2026-10-21

## Status

Accepted. The user asked for frames on windows that need one, without touching the Patina apps
that draw their own, X11 first, and confirmed fullscreen games are never framed.

## Context

The compositor draws no window frames. Wayland apps draw their own, as on GNOME: there's no
xdg-decoration global, so GTK, Qt's plugin, libdecor and Patina's own frame (patina-ui's
`frame.rs`, opened with `with_decorations(false)`) all do. The X window manager in `xwayland.rs`
(ADR-0040) draws none either, so an X11 app that expects the window manager to frame it is a bare
rectangle. The Minecraft launcher (CEF, on X11) is one: no title bar to move it by, no close
button.

Every desktop with title bars frames such windows: Mutter, KWin and sway are all full X window
managers in this respect. An X11 app says it draws its own frame with `_MOTIF_WM_HINTS`
(decorations off), which Smithay reads as `X11Surface::is_decorated()`; Chrome, Steam, Discord,
winit (so Patina apps under X11) and Wine for a window without a Windows title bar
(`WS_CAPTION`) set it.

Blender 5.2 (the snap) on Wayland is also bare here: it was built without libdecor and expects
server-side frames through xdg-decoration everywhere but GNOME. That half is left for later, as
Blender is mostly run maximised; this decision makes it small.

Considered and ruled out:

- **Frames drawn by a client process** (a layer-shell piece per window, like the bar): it would
  have to follow each window's position and stacking from outside, a frame behind every move.
- **Plain rectangles in the renderer**, with a title from a text rasteriser of its own: it would
  look like nothing else on the desktop and need its own skinning.

## Decision

- **An X11 window gets a frame when** it's managed (not override-redirect: menus, tooltips and
  some games), isn't fullscreen (asked for, or sized to the screen, which is taken as fullscreen
  per ADR-0041), and hasn't set `_MOTIF_WM_HINTS` with decorations off. Everything else is drawn
  as now. A window that goes fullscreen loses its frame, and gets it back when it leaves.
- **The compositor draws it, painted with egui:** a title bar with the window's title, and
  maximise and close buttons, in the skin's colours, through patina-shell's CPU painter
  (`raster.rs`), the way the bar is drawn. The desktop has nowhere to minimise to, so there's no
  minimise button. The bar's pixels are uploaded as a texture and drawn above the window by every
  backend; they're painted again only when the title, width, focus, hover or skin change.
- **It acts as a native frame:** dragging the title bar is the compositor's move (snapping and
  tiling included), its edges resize, maximise is the compositor's (`State::maximize`), close is
  `WM_DELETE_WINDOW`.
- **The frame is part of the window's outer rectangle** for placement, tiling, gaps, splits,
  screenshots and the pointer: the X window is configured to the rectangle less the title bar.
- **Wayland comes later, with the same painter:** xdg-decoration answering `server_side` to an app
  that asks, so Blender, Qt apps and mpv get Patina's frame; Patina and libadwaita apps ask for
  `client_side` and keep their own.

## Consequences

- X11 apps without their own frame can be moved, maximised and closed with the mouse, and look like
  the rest of the desktop, following the skin live.
- Patina apps are untouched in both halves: under X11 they set the hint, and on Wayland they ask
  for client-side frames.
- Every place that takes a window's geometry as the client's own (placement, `tiling.rs`,
  `split.rs`, `maximize.rs`, fullscreen, screenshots, hit-testing, moves and resizes) has to
  allow for the title bar: the bulk of the work, and where bugs will be.
- A game that starts windowed with a Windows title bar is framed until it goes fullscreen, as on
  Windows.
- An X11 app that sets no hint but draws a title bar of its own would get two. If one turns up,
  a per-app list of windows never framed goes in the config.
