# patina-clock

The menu of the bar's clock (ADR-0082): a month, and the chosen day's events from the Calendar app.

## On the desktop

From the bar's clock, at the bar's right end: today's date, then a month with ISO week numbers,
today outlined and a dot under each day with something on, then the chosen day's events with
their calendar's colour, time, title and place. A click on a day chooses it, twice opens Calendar
there; a click on an event opens it in Calendar. Arrows move the day, Page Up/Down the month, Home
back to today, Enter opens Calendar. Escape, a click elsewhere or the clock again closes it.

## How it fits in

- **Started by** the bar's clock (its `calendar` command), with the bar's skin and edge. A second
  run closes the open one (patina-shell's `Single`).
- **Calendar keeps the data, the menu draws it.** The events come from `calendar --agenda --from
  DATE --days 42` (a month's grid), JSON read from Calendar's cache without opening it
  (Calendar's ADR-0009); `calendar --open DATE [--event UID]` opens it, started through the
  compositor's `Spawn` like the bar's commands (ADR-0045). The agenda is as fresh as Calendar's
  last sync.
- Without Calendar on the PATH, it shows the month and says so.

## Code

`main.rs` (one copy, the thread running Calendar, the surface), `ui.rs` (the month, the day's
events, the keys), `agenda.rs` (reading `--agenda`, the `--open` command line).

## Running

```sh
cargo run -p patina-clock
PATINA_CLOCK_PREVIEW=dir cargo test -p patina-clock preview -- --ignored   # each skin, as PNGs
```
