# 0082. The clock opens a month with the day's events, which the Calendar app supplies

Date: 2026-10-23

## Status

Accepted. The user asked what a click on the clock should do, doubting time settings. They chose a
month with the coming events, and that the desktop draw the menu while their Calendar app
(`../calendar`) supplies the data. The menu's design is mine.

## Context

The bar's clock was a label; clicking it did nothing. Desktops differ on what it opens. Date and
time settings are rarely needed, since NTP keeps the time. A month and what's on is what people
reach for.

The events live in the Calendar app, a Patina app syncing with the user's CalDAV server. Two ways
to put them under the clock were considered:

- **A menu mode in Calendar.** Calendar is an eframe window: it can't anchor to the bar as a
  layer-shell surface, so it would place like any window, and every click would start the whole
  app, its sync and its network.
- **A desktop menu fed by Calendar.** The menu is a piece like patina-wifi, drawn on patina-shell
  (ADR-0011), opening from the clock (ADR-0063). Calendar gives the events through a command,
  `calendar --agenda`, which reads its cache and expands repeating events with its own code
  (Calendar's ADR-0009), and opens itself where asked with `calendar --open`.

## Decision

- **patina-clock** is the clock's menu: today's date, a month in a six-week grid with ISO week
  numbers, today outlined and a dot under days with something on, then the chosen day's events
  with their calendar's colour. Keys move the day and the month; Enter opens Calendar.
- **Calendar supplies the events.** A thread runs `calendar --agenda --from <grid's Monday>
  --days 42` for each month shown, and parses its JSON. A click on an event runs `calendar --open
  DATE --event UID`, through the compositor's `Spawn` (ADR-0045), so it opens that day's
  occurrence. Without Calendar, the menu shows the month alone.
- **The clock is a button** running the bar's new `calendar` command, `patina-clock` by default,
  like the tray's icons. Its tooltip is the date in full.

## Consequences

- The events are as fresh as Calendar's last sync, and `make deploy` doesn't install Calendar: a
  machine needs it installed, and run once, for events to show.
- The agenda's JSON is an interface between two repositories; Calendar's ADR-0009 says to add
  fields, not rename them.
- Each event click starts a new Calendar window, even when one is open, until Calendar is single
  instance.
