# 0009. Other programs read the agenda and open the app through its command line

Date: 2026-10-23

## Status

Accepted. The user wanted the Patina desktop's clock to show what's coming up, with Calendar
supplying the data and the desktop drawing the menu. The command-line shape is my design.

## Context

The Patina desktop's bar shows a clock. A click should open a small month with the coming days'
events, from the edge of the bar like its other menus. Those menus are layer-shell surfaces drawn
by patina-shell. Calendar is an eframe window, which can't anchor to the bar, so a "menu mode"
inside Calendar was ruled out: it would place like any window, and start the whole app (sync
thread, CalDAV) on every click.

That leaves the desktop drawing the menu, and needing the events from here. These were considered:

- **The desktop reads the cache file itself.** It would have to parse iCalendar and expand
  recurrence rules, duplicating `ics.rs` and `model.rs`, and `snapshot.json` would become a
  format kept for someone else.
- **A D-Bus service.** It answers only while the app runs, and Calendar usually isn't running.
- **A command that prints the agenda.** Reads the cache this app keeps (ADR-0003) with this app's
  parsing and expansion, whether or not the window is open.

## Decision

- `calendar --agenda [--from DATE] [--days N]` prints the occurrences on those days as JSON:
  `uid`, `title`, `location`, `calendar` (its name), `color`, `start` and `end` (RFC 3339 in the
  local zone), and `all_day`. It's sorted by day, all-day events first, then by start. Hidden
  calendars are left out, as in the window. It opens no window and touches no network.
- `calendar --open DATE` opens the window at that day. `--event UID` with it opens that event's
  occurrence on that day, so a click on Tuesday's instance of a repeating event edits Tuesday's.
  The event is opened once it's among the events, which may be after the first sync.

## Consequences

- The agenda is as fresh as the app's last sync. A machine where Calendar hasn't run for a week
  shows last week's view of the server. If that matters, `--agenda` could refresh the cache first.
- Each `--open` starts a new window, even if one is open. Single-instance would need the running
  window to listen, likely on D-Bus.
- The JSON fields are now an interface. Add fields freely, but don't rename or remove them
  without updating the desktop.
