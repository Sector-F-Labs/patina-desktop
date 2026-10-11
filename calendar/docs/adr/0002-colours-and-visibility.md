# 0002. Keep calendar colours on the server and which calendars show on this machine

Date: 2026-10-11

## Status

Proposed. The user asked for colours and for showing and hiding calendars, Apple style. Where
each is stored is my choice.

## Context

A calendar's colour can be kept by each client, or on the server. `plan` stores it on the server:
a `.color` file in the calendar's folder, served as Apple's `calendar-color`, and changed by
`PROPPATCH`, so a change carries over to every client.

Which calendars are shown is a different kind of setting. Hiding Suggestions on the desktop
shouldn't hide it on the phone.

## Decision

- **Colours:** Calendar reads each colour from the server. Right-clicking a calendar offers ten
  Apple-style colours and a custom one, and the choice is written back with `PROPPATCH`, so every
  client shows it. A calendar without a colour uses the skin's accent.
- **Visibility:** shown and hidden calendars are a setting of this machine, kept in
  `~/.config/calendar/config.toml` as a list of hidden calendar hrefs. A newly added calendar is
  shown.
- **Drawing:** events are drawn in their calendar's colour.
  - **Week and day views:** a tinted block with a bar in the full colour. The tint is mixed
    towards the skin's panel colour, so it works in light and dark skins alike.
  - **Month view:** all-day events are filled pills with black or white text, whichever reads
    better. Timed events are a coloured dot and text.

## Consequences

- Changing a colour here changes it on the phone and Mac too. That's what Apple Calendar does
  with iCloud.
- A server that doesn't store `calendar-color` keeps showing its own (or no) colour after a
  change. Calendar shows the new colour until the next refresh.
- Hidden calendars are per machine and aren't synced.
