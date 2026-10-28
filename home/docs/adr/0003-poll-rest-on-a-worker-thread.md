# 0003. Poll the REST API from a worker thread

Date: 2026-10-28

## Status

Proposed

## Context

Home Assistant offers a REST API and a WebSocket API that pushes state changes. The UI must never
block on the network. A WebSocket client means an extra dependency, an authentication handshake,
reconnect handling and event merging; the REST API is two simple calls: `GET /api/states` and
`POST /api/services/<domain>/<service>`.

## Decision

We will start with the blocking REST client (`ureq`) on a worker thread (`src/ha.rs`), polling
`/api/states` every 5 seconds and refreshing straight away after a service call. The UI changes a
toggled entity's state at once and lets the next poll confirm it.

## Consequences

- Simple and easy to test; the UI stays responsive.
- State changes made elsewhere show up up to 5 seconds late, and every poll fetches all entities.
- Moving to the WebSocket API later only changes `ha.rs`; the app sees `Update`s either way.
  Revisit if the delay or the load becomes a problem.
