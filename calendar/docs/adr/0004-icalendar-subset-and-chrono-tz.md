# 0004. Read a subset of iCalendar ourselves, resolve zones with chrono-tz, expand rules with rrule

Date: 2026-10-11

## Status

Proposed. These are my choices.

## Context

A calendar client has to read events, expand recurring ones, and write edits back without
losing anything it doesn't understand. On urd:

- 388 event files use `DTSTART`, and 193 use `TZID=Europe/Stockholm`.
- 12 are all-day.
- Most `RRULE`s are inside `VTIMEZONE` blocks (summer-time rules). Real recurring events are
  weekly, with `BYDAY` and `UNTIL`.
- A few files have `EXDATE` and `RECURRENCE-ID` overrides.

Full iCalendar crates exist, but each would have had to round-trip unknown properties exactly.

## Decision

- **Our own reader:** `ics.rs` reads and writes iCalendar as a tree of components and properties,
  unfolding and folding lines and keeping property order, unknown properties and other
  components.
- **Edits:** an edit changes only the dialog's fields (summary, location, notes, start, end),
  bumps `SEQUENCE` and sets `DTSTAMP` and `LAST-MODIFIED`.
  - **Timed events** keep the way their start was written: in a named zone, floating, or UTC.
    A weekly event in Stockholm therefore stays at the same local time across summer time.
  - **New events** are written in UTC with a random UID.
- **Time zones:** resolved by `TZID` name with chrono-tz, including the
  `/mozilla.org/…/Europe/Berlin` spelling. `VTIMEZONE` blocks are kept but not read.
- **Recurrence:** expanded with the `rrule` crate, given only `DTSTART` and `RRULE`. `EXDATE`s and
  `RECURRENCE-ID` overrides are matched by instant in our code, so their many spellings never
  reach `rrule`'s parser.
- **Deleting one occurrence** of a series adds an `EXDATE` written the way its `DTSTART` is.
  Deleting a changed occurrence removes just that `VEVENT` from its file.

## Consequences

- Files written by other clients survive an edit here, unknown properties included.
- A zone chrono-tz doesn't know, defined only by its `VTIMEZONE` block, falls back to local time.
- **Not supported:** editing one occurrence of a series. Changes apply to every occurrence, and
  the dialog says so. `RDATE` is also not read.
- **Revisit** if events show at wrong times from unusual zones, or if editing single occurrences
  becomes a need.
