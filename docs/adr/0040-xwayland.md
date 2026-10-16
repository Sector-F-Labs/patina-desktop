# 0040. X11 apps run through Xwayland, with the compositor as their window manager

Date: 2026-10-17

## Status

Accepted

## Context

Steam wouldn't open on the Patina desktop. Its client is 32-bit and built on an old Chromium
without Wayland, so it only talks to an X server; Discord, older Electron apps, most games and Wine
are the same. Every other Wayland desktop runs **Xwayland**, an X server that is itself a Wayland
client, for them. The compositor had none, and cleared `DISPLAY` for its clients.

The user first thought of this as a portal. Portals are D-Bus services for sandboxed apps (Files
is already the session's file chooser through one) and don't put X11 windows on screen.

Smithay has Xwayland support behind its `xwayland` feature: `XWayland::spawn` starts the server
on a free display, and `X11Wm` is the window manager side, calling an `XwmHandler` for each X
window's map, configure, move and resize requests. X windows don't speak xdg-shell, so the
compositor has to grant those requests itself. Keyboard focus also has to be set on the X side
(`SetInputFocus`); Smithay's `X11Surface` does that as a keyboard focus target, which the
compositor's `WlSurface` focus couldn't express.

An external bridge, xwayland-satellite (as niri uses), was considered. It turns X windows into
ordinary xdg toplevels, so the compositor needs almost nothing. But it's one more program to
package on Ubuntu and FreeBSD, and it's young. Smithay's built-in window manager keeps it all
inside the compositor.

## Decision

- The compositor builds Smithay with `xwayland`. At start-up it spawns `Xwayland` (if installed)
  on a free display and gives every client `DISPLAY` beside `WAYLAND_DISPLAY`. It exports
  `DISPLAY` to the session along with `WAYLAND_DISPLAY`. Without Xwayland, it logs that, and
  Wayland apps are unaffected.
- When the server is ready, `X11Wm` starts, and `xwayland.rs` is its handler:
  - A managed window asking to map is shown as a Smithay `Window` in the same space as Wayland
    windows. It's placed where it asked if it gave a position (Steam's dialogs do), otherwise
    cascaded like any new window, and focused.
  - Override-redirect windows (X menus, tooltips, popups) are shown where they put themselves,
    and never focused.
  - Size requests are granted. Windows that are already shown keep their place; the compositor
    places them.
  - `_NET_WM_MOVERESIZE` (Steam's own title bar) starts the compositor's move grab, or an X11
    resize grab.
  - Moving a window by any means tells the X server where it now is, so its menus open in the
    right place.
- The seat's keyboard focus becomes `Focus`: a Wayland surface or an X11 surface. Focusing an X
  window sets the X input focus, and its Wayland surface gets the keys.
- X windows join the bar's tabs (their title, their class as the app), Alt+Tab, Super+W (a
  `WM_DELETE_WINDOW` close) and Super+Q.
- Later: copy and paste between X and Wayland (Smithay bridges selections, but it isn't wired up
  yet), maximise and half-screen tiling for X windows, and X cursors in the compositor's theme.

## Consequences

- Steam, Discord, Wine and other X11-only apps open on the Patina desktop.
- X11 apps are drawn at scale 1 and stretched on a scaled screen, so they're soft at 1.5×. GNOME
  makes the same trade by default.
- Xwayland runs from start-up even if no X app is ever opened, at a few tens of MB.
- Windows now come in two kinds. Code that reaches into a window's xdg toplevel (maximise,
  keyboard resize, the bar's titles) needs an X11 path, or leaves X windows out, as maximise does
  for now.
- Until the clipboard is bridged, copying between an X app and a Wayland app doesn't work.
