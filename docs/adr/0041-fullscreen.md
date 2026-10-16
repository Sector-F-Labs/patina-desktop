# 0041. Fullscreen windows cover the whole screen, over the bar, while they have the keyboard

Date: 2026-10-17

## Status

Accepted

## Context

Half-Life, launched from Steam under Xwayland (ADR-0040), opened as a small window, not filling
the screen or even the size the game drew at. Games ask the window manager for fullscreen with
`_NET_WM_STATE_FULLSCREEN`. Older ones instead ask for the screen's size at its top-left corner.
The compositor ignored both, and Wayland's `xdg_toplevel.set_fullscreen` too. So the game kept its
first small size, and position requests from a shown window were refused. Maximising (ADR-0014)
fills only the work area beside the bar, which isn't what a game or a video player wants.

## Decision

- A window asking for fullscreen covers the whole output: Wayland's `set_fullscreen`, X11's
  fullscreen request (before or after it's shown), or an X window asking to be exactly the
  screen's size at its corner. It's sized to the output (xdg's Fullscreen state, or an X
  configure and `_NET_WM_STATE`), placed at the output's corner, focused, and remembered as
  `State::fullscreen`, with where it was before.
- While it has the keyboard, it's drawn above the layer surfaces (the bar) in every backend, and
  the pointer reaches it before them. Focusing another window, with Alt+Tab or the bar, shows the
  desktop as usual, with the fullscreen window underneath.
- Leaving fullscreen (the app's request) puts it back where it was. A second window going
  fullscreen takes the place from the first, which goes back.
- Super, the launcher and the lock screen still work over it. The overlay layer (menus, the volume
  flash) is drawn above it.

## Consequences

- Games and video players fill the screen as on GNOME.
- Only one window is fullscreen at a time.
- X games see the screen at the compositor's logical size, and under a scale above 1 they're drawn
  at that size and stretched (ADR-0040). A game picking a lower resolution through RandR isn't
  emulated. It renders at the size it's given.
- There's no key to make a window fullscreen yet (GNOME has none by default either); apps ask.
