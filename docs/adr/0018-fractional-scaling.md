# 0018. Fractional scaling through wp_fractional_scale and wp_viewporter

Date: 2026-10-14

## Status

Accepted

## Context

On the user's ThinkPad X1 Carbon (a 14" 1920x1200 panel) the hardware backend picks scale 1.5
(ADR-0016). A plain `wl_output.scale` is an integer, so clients only learned "2", drew at twice the
size, and the compositor shrank the result to 1.5: everything worked but text was soft. The user
chose fractional scaling as the next step after the first run on the laptop.

## Decision

- The compositor offers `wp_fractional_scale_v1` and `wp_viewporter`. Each surface's preferred
  scale is the output's exact scale, set when the client asks for it and again whenever the output's
  scale changes. Smithay's renderer already honours viewports, so a client's 1.5x buffer is drawn
  one pixel to one pixel.
- patina-shell (the bar and the launcher) uses them where the compositor offers them: each surface
  draws `round(size x scale)` pixels, sets its viewport to its logical size and a buffer scale of
  1, and tells egui the exact `pixels_per_point`. Without them it keeps integer buffer scales.
- `PATINA_SCALE` also sets the nested compositor's scale, so fractional scales can be tried on any
  desktop.

## Consequences

- Clients that speak the protocol render sharp at 1.5: winit (so eframe apps such as Teleprinter
  and UnAmp), foot, GTK4, Qt 6, Firefox and Chromium, and the bar and launcher.
- GTK3 apps don't speak it and stay at 2x shrunk.
- The scale is the single output's; with more than one monitor, each surface will need the scale of
  the output it's on.
