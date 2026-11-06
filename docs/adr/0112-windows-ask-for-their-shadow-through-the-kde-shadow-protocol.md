# 0112. Windows ask for their shadow through KDE's shadow protocol, and the compositor draws it

Date: 2026-11-06

## Status

Accepted. The user wanted Patina apps to have a shadow like Firefox's, asked for it to be
prototyped with the KDE shadow protocol, then for the call to go into patina-ui's frame code
(Patina's ADR-0030). The prototype ran in the Linux VM with Settings; maximised and tiled windows,
a second app, KWin and the hardware's HiDPI scales are untested.

## Context

Firefox, GTK, Qt and Telegram draw their own shadow on Wayland: the window's surface is bigger
than the window, the shadow is in the margin, and the client tells the compositor where the real
window is with `xdg_surface.set_window_geometry`. A Patina app draws its own frame (no system
decorations, a one-pixel border, rounded corners) and no shadow. Nothing adds one: in a headless
GNOME Shell (Ubuntu 26.04) `patina-settings` was flat, with no shadow on any side. So every
window on our desktop has a shadow except our own apps.

Options considered:

- **The compositor shadows every window it doesn't know has one.** It would have to tell, per
  window, whether the client drew a shadow: a list of apps, or comparing the surface with the
  window geometry. The list is manual and the comparison depends on every client reporting its
  geometry and on the state it is in, so a mistake is a double shadow. Rejected.
- **Patina apps draw the shadow in a margin, like GTK.** It works on every desktop, but needs
  `set_window_geometry` with the margin left out, and winit calls it itself with the whole surface
  and has no API to change it (rust-windowing/winit#3438, open since 2024, labelled hard, and
  the reason libcosmic has no shadows either). Without it, tiling and snapping count the margin as
  window: a tiled window would sit a shadow's width inside its tile. It means patching winit in
  every app. Not now.
- **The compositor draws the shadow for windows that ask,** with `org_kde_kwin_shadow`: the
  client hands over eight tiles (four corners, four sides) and how far they reach, and the
  compositor draws them outside the window. The surface stays the window's size, so there is no
  geometry problem and no winit patch. KWin implements the same protocol. GNOME doesn't.

Smithay has no implementation of it; `wayland-protocols-plasma` has the definition.

## Decision

- **The compositor offers `org_kde_kwin_shadow_manager` (version 2)** (`compositor/src/shadow.rs`)
  and draws what a client commits.
  - The eight tiles are shm `ARGB8888` buffers of at most 512 pixels a side. They are copied out
    when the client commits the shadow and the buffers released at once.
  - Offsets are how far the shadow reaches outside the window's geometry. The corners sit at the
    corners of that rectangle and the sides are stretched between them, as KWin lays them out.
  - It is drawn under its window and above the windows below, in `frames::desktop_elements`. A
    fullscreen or maximised window gets none, and a client can `unset` or destroy it.
  - It is drawn at 45% while its window isn't the focused one (`Activated`), as Firefox's is: on a
    white background its shadow is 27 to 34 of 255 dark unfocused against 61 to 79 focused. The
    compositor does this for every client, so apps need nothing. It can't shorten the reach as
    Firefox also does (about 12 px against 20), only lighten it.
- **`[shadows] enabled`** in the compositor's config turns them all off (on by default), and
  Settings' Appearance page has the checkbox. The renderer reads it from a flag the config sets,
  and a reload applies it within a second.
- **A window that doesn't ask gets nothing.** Firefox, GTK and Qt windows that draw their own
  keep it and don't get a second.
- **The shadow takes effect when the client commits it,** not with the surface's next commit as
  KWin has it.
- **Patina apps ask through patina-ui** (Patina's ADR-0030).

## Consequences

- Patina apps look like the rest on our desktop and, with KWin, on KDE.
- On GNOME, and on any compositor without the protocol, Patina apps stay flat. Closing that gap
  needs the margin and the winit change above, and nothing here prevents it: an app could use the
  margin only where the protocol is missing.
- Windows the compositor frames itself (X11 windows, and Wayland ones that take server-side
  decorations, ADR-0077 and ADR-0085) get no shadow yet. They never ask, so they would need the
  compositor to add one, which is cheap to add here: it knows they have none.
- The HUD's parts are layer surfaces and ask for theirs through the same protocol (ADR-0113); the
  menus don't yet.
- The shadow is a fixed picture scaled with the window, not blurred per size. At a fractional
  output scale the tiles are stretched like any buffer, so they soften.
- Not checked: maximised and tiled windows, a second app, and KWin. Applying the shadow at its
  own commit instead of the surface's means a resize and a changed shadow can show for a frame
  out of step.
