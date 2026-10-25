# 0092. Screen capture for apps, through the portal, and the desktop's own recording

Date: 2026-10-25

## Status

Proposed. The user found OBS couldn't see the screen, and asked for capture to work for other
programs and for recording to be a choice in the Print key's menu (ADR-0091).

## Context

Nothing outside the compositor could capture the screen (ADR-0034): no wlr-screencopy, no
ext-image-copy-capture. On Wayland, OBS, browsers sharing a screen (Meet, Discord, Zoom) and the
like ask xdg-desktop-portal for a ScreenCast. The portal passes that to a backend for the desktop,
which gets frames from the compositor and streams them over PipeWire. The Patina session's portal
config (Files' `patina-portals.conf`) named no ScreenCast backend, and the compositor had nothing
to give one.

Ubuntu 26.04 has xdg-desktop-portal-wlr 0.8.1, which captures screens and windows through
ext-image-copy-capture-v1, choosing windows from ext-foreign-toplevel-list-v1. Its Screenshot
portal runs `grim`, which needs wlr-screencopy. Smithay has ext-image-copy-capture on its master
branch, not in 0.7.0, which is still its last release; wf-recorder and grim in Ubuntu speak only
wlr-screencopy.

Considered and ruled out:

- **wlr-screencopy, written here.** Smithay will never have it; it captures whole screens only,
  and ext-image-copy-capture replaces it.
- **A portal backend of our own in the compositor, streaming to PipeWire** (as niri does). The
  most work, and nothing it gives that xdg-desktop-portal-wlr doesn't, for now.
- **Recording through wf-recorder.** Ubuntu's 0.6 has no ext-image-copy-capture.
- **Recording only, without the portal.** OBS and screen sharing would still not work.

## Decision

- **Smithay's capture modules are backported** to our fork's `patina-0.7` branch (ADR-0044) as
  they were before upstream's Dispatch2 change, with the session's client getter, but without
  upstream's `duplicate_frame` error (5ed59867): xdg-desktop-portal-wlr 0.8.1 has two frames in
  flight at the start of a stream, which the protocol forbids, and was disconnected for it. A
  session keeps every frame until it's destroyed, and the compositor answers them all.
- **The compositor serves ext-image-copy-capture** for the screen and for single windows
  (`compositor/src/capture.rs`), and **ext-foreign-toplevel-list** for the windows to choose from
  (`toplevels::refresh`, beside wlr-foreign-toplevel-management).
- **A frame waits for something new**: each session has a damage tracker, and the frame a client
  asks for is answered only once the scene has changed since its last (the first at once). The
  scene is drawn offscreen as screenshots are, read back and copied into the client's shm buffer
  (Argb8888 or Xrgb8888). No dmabufs yet. The screen is drawn as it's shown (the pointer on the
  hardware, the locker when locked); a window with its frame, wherever it is. A window's
  sessions stop when it closes; a minimised window's frames wait.
- **Any client may capture**, as on wlroots desktops; the portal's chooser is where an app is
  told no.
- **The portal:** `make install` puts `patina-portals.conf` in `/etc/xdg/xdg-desktop-portal`
  (`$(PREFIX)/etc/xdg` on FreeBSD), which comes before Files' copy in `/usr/share`, with Files'
  line kept: ScreenCast to `wlr`, FileChooser to `patina` then `gtk`, the rest to `gtk`. And
  `/etc/xdg/xdg-desktop-portal-wlr/Patina` makes `patina-screenshot --share` the chooser
  (`chooser_type=dmenu`): a panel in the middle of the screen listing the screen and the
  windows, as the portal gives them on stdin, and printing the line chosen.
  xdg-desktop-portal-wlr is a package to install, as Xwayland is (`make deps`).
- **The desktop records the screen itself**: Record Screen and Record Selection in the Print
  key's pill (`compositor/src/capture/record.rs`). Each pass of the event loop, at most thirty
  times a second and only when the screen has changed, the compositor draws the screen with the
  pointer, cuts out the selection (even sizes, for the video's colour) and hands the pixels to a
  thread that feeds `ffmpeg` on stdin. ffmpeg stamps frames as they arrive and writes an H.264
  MP4 at a steady 30 frames a second to `~/Videos/Screen Recordings`, named as macOS names them.
  No sound yet.
- **While recording**, `patina-screenshot --recording` shows a pill at the bottom of the screen
  (a breathing red dot, the time, Stop), left out of the recording by its namespace. Stop there,
  or the Print key again, ends the recording; the compositor closes the pill's stdin.

## Consequences

- OBS's PipeWire screen capture, and browsers' screen sharing, work through the portal, for the
  screen or one window.
- Screen recordings need no other program than ffmpeg, which `make deps` names.
- Every captured frame and every recorded one is a copy through memory, as screenshots are; a
  1080p screen changing all the time costs a few percent of a CPU core more than drawing it.
  Dmabufs, and the GPU's video encoder, are for later.
- The fork carries more than fixes now; when a Smithay release has the modules, the
  `[patch.crates-io]` can go, once the compositor follows Smithay's Dispatch2 change.
- The portal config moves from Files to the desktop: which backend serves what is the session's
  decision. Files' copy stays as a fallback.
- Screenshots through the portal (`grim`) still don't work: the Print key covers taking them.
