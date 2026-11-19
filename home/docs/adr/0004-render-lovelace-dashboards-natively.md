# 0004. Render the user's Lovelace dashboards natively

Date: 2026-10-28

## Status

Accepted

## Context

The user wants Home to show the dashboards they already built in Home Assistant, not a second layout
to maintain. Lovelace configs are served only over the WebSocket API (`lovelace/dashboards/list`,
`lovelace/config`), not REST. The options were a webview of HA's own frontend (not native, can't take
Patina skins), a dashboard Home designs itself (the user chose against it), or reading the Lovelace
config and drawing its cards in egui.

## Decision

We will fetch the dashboards over a one-shot WebSocket connection (`tungstenite`, `src/ha.rs`) when
Home connects and on View → Refresh, and draw them in `src/dashboard.rs`. Live states and service
calls stay on the REST worker of [0003](0003-poll-rest-on-a-worker-thread.md), which this leaves in
place. The first dashboard opens by default, until the user picks the entity list. The cards drawn
are `entities`, `tile`, `button`, `entity`, `sensor`, `light`, `glance`, `markdown` (as plain text),
`heading` and the `horizontal-stack`, `vertical-stack` and `grid` containers; masonry, sections and
panel views are laid out in columns. `gauge` and `thermostat` show as plain entity rows. Any other
card shows its type name rather than disappearing. An auto-generated default dashboard, which has no
stored config, is skipped.

## Consequences

- The user's own dashboards appear in Home with Patina skins, and editing them stays in Home
  Assistant: Home doesn't write configs back.
- Unsupported cards (camera, history graph, media player, conditional and more) show as
  placeholders; each is added when it's missed.
- Dashboards load once, not live: a change made in Home Assistant appears on the next Refresh.
- Two network paths (REST and WebSocket) and a `tungstenite` dependency with rustls. Revisit
  together with 0003: a persistent WebSocket would serve states, calls and dashboards.
