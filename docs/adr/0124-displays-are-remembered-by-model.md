# 0124. Displays are remembered by model, and listed in Settings

Date: 2026-11-08

## Status

Accepted

Refines how [0121](0121-multi-monitor.md) knows a monitor.

## Context

ADR-0121 remembered where a monitor sits by its id: maker, model and serial from its EDID, so it
comes back wherever it's plugged in. That is right at a desk of your own and wrong at a hot desk,
where the monitors are all the same model and none is the one you placed: each is a new monitor,
and each asks to be placed again.

It also gave no way to see what was remembered, or to make it forget.

## Decision

- **A monitor goes by an entry saved for its own id if there is one, else by an entry of its model**
  (the id without the serial: `GSM-772E` for `GSM-772E-0005F79F`), the one saved last first.
  Each entry is used by one connected monitor at most, so two new Dells at a hot desk take the two
  places two Dells were given, and a third has none and is new. A monitor with its own entry keeps
  it, whatever else is connected.
- **Only a placement saves an entry**, for the monitor placed. Plugging into a hot desk, and
  extending onto what the model's entry puts in place, save nothing, so desks don't pile up
  entries. Placing a monitor that went by its model's entry saves its own, and that becomes the one
  the model's next monitor goes by.
- **A neighbour is a connected monitor, not an id.** What an entry is next to is the monitor with that
  id if it's connected, else the one that goes by that monitor's entry. So a Dell kept as "right of
  the other Dell" is right of whichever Dell goes by that entry, at any desk. A monitor whose
  neighbour isn't connected has no place and goes after the one before.
- **The compositor lists what it remembers** (`Saved()` on `com.sectorflabs.Patina.Displays`: id,
  name when saved, whether a connected monitor goes by it, where it was put) and forgets an entry
  (`Forget(id)`). Settings' Displays page shows them, newest first, grayed when none is in use, each
  with Forget. Names are kept in `displays.toml` so a monitor that isn't connected can be shown.
- A display that goes by a saved entry is **known**: plugging it in opens the popup as before, but
  extending onto it doesn't open Settings.

## Consequences

- A hot desk's monitors come up where yours were, with one choice in the popup, and nothing to place.
- Two monitors of one model at one desk can't be told apart at a desk you haven't placed them at:
  they take the places in the order they are found, which can swap them. Moving one fixes it, and
  is remembered by its serial.
- The list only grows by placing monitors; forgetting is by hand, one entry at a time.
- `displays.toml` entries from before have no name and are shown as their model until placed again.
