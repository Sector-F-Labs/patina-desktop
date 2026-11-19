# 0001. Build Calendar as a native egui app on Patina, for plan on urd

Date: 2026-10-11

## Status

Accepted

## Context

The user's calendars live in `plan`, their own CalDAV server, running on urd and reached as
`https://plan.divanv.com`. Their phone, Mac and GNOME clients sync with it. The user asked for a
desktop calendar in the Patina family, called Calendar, private and under Sector F Labs. It should
have:

- day, week and month views, with a Today button;
- calendars in colour that can be switched on and off in a sidebar, "just like Apple Calendar on
  desktop or GNOME Calendar".

GNOME Calendar and Evolution already speak CalDAV, but they don't look or behave like the Patina
apps, and they don't take Patina skins.

## Decision

We will build Calendar with egui/eframe on [Patina](https://github.com/Sector-F-Labs/patina)
(`patina-ui`):

- **Patina's parts:** its own window frame, with File, View and Skins menus at the left of the
  header bar and the view controls at the right (‹ Today › and Day, Week, Month). There's a
  "System title bar" setting as the escape hatch.
- **Light/dark and skins:** the desktop's light/dark setting followed through
  `patina::appearance`, and skins through `patina::skin`: Default, Dark Orange and the shared
  `~/.config/patina/skins/`.
- **Stock egui style and fonts** otherwise. The views are painted, with every colour taken from
  the active visuals, except the calendars' own colours.
- **Speaking standard CalDAV**, not anything `plan`-specific: discovery through
  `current-user-principal` and `calendar-home-set`, `PROPFIND`, `REPORT`, `PUT`, `DELETE`, and
  `PROPPATCH` for colours. Any CalDAV server should work, and `plan` is the one it's tested
  against.
- **A sidebar like Apple's and GNOME's:** a mini month for jumping to a date, then the calendars.
  Each has a checkbox in its colour; clicking toggles it, and right-clicking picks a colour.
- **Location and remote:** `com/sectorflabs/calendar`, private, with urd as its remote.

## Consequences

- Calendar looks and behaves like the rest of the Patina family and takes the same skins.
- It moves to a new egui version only together with Patina.
- **Ours to keep up:** the calendar views, recurrence and iCalendar handling. ADR-0002 and
  ADR-0004 keep that small.
- Task lists (`VTODO` calendars) are left out; this is a calendar of events.
