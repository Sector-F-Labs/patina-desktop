# 0130. Whether an app's maximise fills inside the gaps is a setting of its own

Date: 2026-11-09

## Status

Accepted. The user said the two should be separate: turning gaps on shouldn't take maximising away.
Amends [ADR-0093](0093-with-gaps-maximising-fills.md).

## Context

ADR-0093 made an app's maximise a fill inside the gaps whenever gaps are on, never telling the app
it's maximised, so that a maximised window looks like the others: corners, shadow and the gap. The
price is that with gaps on there is no real maximise: no edge to edge, and the app's button never turns
into restore. Someone who wants gaps around tiled windows and the floating bar may still want a
maximise that covers the screen, and the two had one switch.

## Decision

- **`[gaps] maximise_inside`**, true by default, which is ADR-0093's behaviour. With it false, gaps
  only affect tiling (halves, Super+Up) and the floating bar, and an app's maximise is the real one
  of ADR-0052: the work area edge to edge, the `maximized` state sent.
- **One place decides it**: `State::maximize`, through `Config::maximise_fills()`, which is true
  when there's a gap to fill inside and the setting is on. Every way a maximise arrives goes there
  (ADR-0093), so none needs its own check.
- **Settings' HUD page**, in the Gaps section, has "An app's maximise fills inside the gaps", enabled
  while gaps are.
- **Nothing changes by default.** Super+Up and dragging to the top still fill inside the gaps either
  way.

## Consequences

- A user can have gaps and a real maximise. A maximised window then covers the gaps and the floating
  bar's margin, and the app squares its corners, as on any desktop without gaps.
- The setting is read as the config reloads. A window already placed stays as it is, as with turning
  gaps on (ADR-0093).
