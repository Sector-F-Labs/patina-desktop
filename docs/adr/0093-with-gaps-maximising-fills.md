# 0093. With gaps on, an app's maximise fills inside the gaps, unannounced

Date: 2026-10-25

## Status

Accepted: tested on nifl. Amends [ADR-0052](0052-only-the-app-maximises.md) for gaps mode.
Made optional by [ADR-0130](0130-maximise-inside-the-gaps-is-a-setting.md): `[gaps] maximise_inside`.

## Context

ADR-0052 kept the app's own maximise (its button, a double click on its title bar, the bar's
window menu) a real maximise: the work area edge to edge, gaps or not, with the xdg `maximized`
state. Tiling up (`Fill`) fills inside the gaps, unannounced, so the window keeps its rounded
corners.

With gaps on, the user wants maximising to look like filling. A maximised window covering the
gaps breaks the look gaps mode is for. Changing the Patina apps' frames wouldn't help: Firefox,
GTK and the rest draw a maximised window square because they're told it's maximised.

Size and states are separate in a `configure`: the compositor answers `set_maximized` as it
chooses. Two answers were considered:

- **Fill inside the gaps and send `maximized`.** Correct by the protocol, and the app's button
  shows restore. But the app squares its corners and drops its shadow, so a square window
  floats inside the gap: the look ADR-0052 got away from.
- **Fill inside the gaps without `maximized`.** The window keeps its corners. The app doesn't
  know it's maximised, so its button still shows maximise, and a second click sends
  `set_maximized` again, not `unset_maximized`.

## Decision

- **With gaps on, the app's maximise is a `Fill`**, the same placement as Super+Up, and the
  `maximized` state is never sent.
- **Maximise asked again while the window fills restores it**, as its button would have if it
  showed restore. That holds for a window filled by Super+Up too.
- **Without gaps, nothing changes:** a real maximise, as in ADR-0052.

This applies to every way a maximise arrives: xdg `set_maximized`, X11's `_NET_WM_STATE`, the
compositor's own frames (ADR-0077, ADR-0085) and wlr-foreign-toplevel from the bar.

## Consequences

- In gaps mode, a maximised window looks like every other window: corners, shadow, the gap.
- The protocol is bent: apps never see `maximized` while they fill the screen. Their maximise
  button never turns into restore, and an app that remembers being maximised won't.
- An app that sends `set_maximized` more than once for one click would toggle back. None seen.
- Turning gaps on doesn't change a window maximised before; it stays edge to edge until restored.
