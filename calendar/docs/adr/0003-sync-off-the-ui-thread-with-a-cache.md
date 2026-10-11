# 0003. Sync on a worker thread, refetching only changed calendars, with a disk cache

Date: 2026-10-11

## Status

Proposed. Patina's design language requires the UI never to block. This design is mine.

## Context

Every server call can take a while or fail: discovery, the calendar list, each calendar's events,
and saves. The app should open instantly with what it knew last time, and keep working while the
server is slow or away.

## Decision

- **Worker thread:** a thread (`sync.rs`) owns the CalDAV client. The UI sends commands (refresh,
  save, delete, set colour, change account), and the thread sends back updates (busy, synced
  data, error) and asks egui to repaint.
- **Batches:** commands that arrive together run together, then the worker refreshes once, so the
  view shows what the server stored.
- **Refetching:** a refresh fetches the calendar list, then the events only of calendars whose
  `getctag` changed. It runs every five minutes while the app is open, and on File → Refresh.
- **Disk cache:** the last fetched data goes to `~/.cache/calendar/snapshot.json` (written to a
  temporary file, then renamed), and loads at start-up.
- **Edits apply locally first:** saves and deletes change the local copy at once and are sent to
  the server in the background. Errors show in a bar at the bottom, and the next refresh shows
  the server's state.

## Consequences

- The app opens instantly, and works read-only offline, from the cache.
- Edits feel instant. A failed save shows an error, and the next refresh puts the event back the
  way the server has it.
- The cache holds the calendar contents in plain text in the user's cache folder.
- `plan` ignores `If-None-Match`, so a new event can't be refused for clashing with an existing
  file. New events get random UUID file names, so that never happens in practice.
