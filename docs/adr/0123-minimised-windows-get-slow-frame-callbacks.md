# 0123. Minimised windows still get frame callbacks, slowly

Date: 2026-11-07

## Status

Accepted

Adds to [0084](0084-minimise-to-the-bars-tabs.md), which took a minimised window out of the space.

## Context

A minimised window is out of the space, so the render loop never draws it and never sends it a frame
callback. Nothing in the Wayland protocol forbids that: a hidden surface doesn't need to be drawn.

But winit, under egui and eframe, takes the callback as its clock. On Wayland it asks for one each
time it presents a frame, and holds back every `RedrawRequested` until the callback arrives
(`wayland/event_loop/mod.rs`: `if frame_callback_state() == Requested { return None }`). A window that
never gets one never redraws, so the app's `update` or `logic` never runs again, and whatever waits
for it waits: a message from another thread, a timer, a track ending.

UnAmp showed it. Minimised and playing, it kept playing (the audio is another thread), but the bar's
Pause did nothing until the window was brought back, and then it paused at once. UnAmp had done the
right things: D-Bus on a thread of its own, a command channel, `request_repaint()` on every
command, a repaint every 250 ms while playing. The main thread was idle in winit's event loop, not
blocked, and had simply not been told it may draw. Every egui app is the same.

## Decision

- **The compositor sends each minimised window a frame callback every 500 ms**
  (`State::start_minimized_frames`, a timer on the event loop). The window is not drawn and nothing
  is composited; the callback is only its permission to run a frame.
- Half a second is the most a control waits to be heard by a minimised app, and the cost of the
  idle apps: a few frames a second drawn into a buffer nobody sees.

## Consequences

- Media controls, a track ending, a download finishing, a terminal's output and anything else an app
  does from its UI loop go on while it is minimised, at up to 2 Hz.
- A minimised app now wakes twice a second. A minimised game would run its frame loop at that rate,
  not at full speed, and not stopped.
- An X11 window gets the same; Xwayland passes callbacks on for the X app's own use.
- Windows on another workspace or hidden some other way would need the same, if there were any: there
  is one desktop, and minimising is the only way a window leaves the space.
- A toolkit that doesn't wait for frame callbacks never needed this, and isn't hurt by it.
