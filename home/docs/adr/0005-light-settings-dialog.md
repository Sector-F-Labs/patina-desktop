# 0005. A settings dialog for lights

Date: 2026-10-28

## Status

Accepted

## Context

In Home Assistant's own dashboards, clicking a light opens its "more info" settings: brightness,
colour temperature, a colour picker and effects. The user wants the same in Home. Home Assistant
doesn't expose a gradient editor for multi-colour lights (Hue gradient strips) through its API; what
it does expose is `effect_list` and `effect`, plus one `hs_color`.

## Decision

We will open a window for a light when its name is clicked, in the entity list and on dashboards
(`src/light.rs`). It shows only what the light's `supported_color_modes` allow: power, a brightness
slider, a colour temperature slider (over a warm-to-cool strip), a hue/saturation picker and an
effect list. A slider or the picker sends `light.turn_on` when it's let go, not while it's dragged,
and the new value is shown at once until the next poll. Service calls now carry data
(`ServiceCall`), so other entities' settings can be added the same way.

## Consequences

- A light's colour, temperature and effects can be set from Home, as in Home Assistant's dashboard.
- Gradients are only reachable as effects, and a gradient light's per-segment colours can't be edited.
- The picker sets hue and saturation (`hs_color`); lights that report only xy or rgb are set through
  that conversion in Home Assistant, which is lossy at the edges.
- Only lights open a settings window so far. Climate, covers, media players and the rest are next
  when missed.
