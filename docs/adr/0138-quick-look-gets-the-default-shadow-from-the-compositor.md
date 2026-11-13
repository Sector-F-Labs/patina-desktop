# 0138. Quick Look's window gets the default shadow from the compositor

Date: 2026-11-13

## Status

Accepted. The user asked for Files' preview windows to throw a shadow, as the app's own window
does. It follows [0112](0112-windows-ask-for-their-shadow-through-the-kde-shadow-protocol.md).

## Context

Patina apps ask for their shadow through `patina::frame::shadow::request`, which needs the window's
`wl_surface`, taken from eframe's `Frame` (ADR-0112). Files' Quick Look isn't a window of its own
in egui's terms but an immediate viewport of the folder's, which eframe makes and gives no
handle to: `Frame` is the folder window's, and a viewport's callback gets only a `Ui`. So the
folder had a shadow and its preview had none.

Ways to give it one:

- **Get the viewport's surface** from winit. Not reachable through eframe or egui.
- **Make Quick Look a window of its own**, a process or an eframe app, so it has a `Frame`. It would
  stop sharing the folder's state (the selection, keys and previews) that ADR-0008 chose it for.
  Rejected for a shadow.
- **The compositor adds the default shadow** to a window that can't ask. ADR-0112 declined to
  shadow every window it doesn't know has one, since a window that draws its own would get two;
  that's a worry for a list that's open-ended, not for one named window of an app of ours that
  we know draws none. Chosen.

## Decision

- **`UNABLE_TO_ASK`** in `compositor/src/shadow.rs` lists the app ids of windows that get the
  default shadow when they haven't asked: `com.sectorflabs.Files.QuickLook`. It is the shadow
  patina-ui makes for `request` (`shadow::tiles` with the default style), made once, and drawn as
  any other, lighter when the window isn't focused, and none when it's maximised or the
  compositor's `[shadows]` is off. A window on the list that does ask gets what it asked for.

## Consequences

- Quick Look looks like the folder window it belongs to. On nifl, with it open, the pixels just above
  its top edge are about 6 of 255 darker, fading out over 16 px, as for the other windows.
- The default style is patina-ui's, not the HUD's `shadow_*` settings (ADR-0113), so changing those
  doesn't reach it, as it doesn't reach windows that ask.
- Another viewport window of ours needs its app id added here; a window that can ask should ask.
- If eframe ever gives a viewport's handle, Quick Look can ask and leave the list.
