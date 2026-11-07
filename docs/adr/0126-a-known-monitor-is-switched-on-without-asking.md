# 0126. A known monitor is switched on without asking

Date: 2026-11-08

## Status

Accepted

Changes when the popup of [0121](0121-multi-monitor.md) opens, and builds on [0124](0124-displays-are-remembered-by-model.md).

## Context

ADR-0121 left every monitor plugged in off until someone chose for it, and opened the popup each time.
That is right for a monitor never seen. For one the user has already placed, it asks what is already
answered: the place is saved, extending was what they chose, and the popup is a click in the way of a
desk that should simply come up as it was. ADR-0124 made that worse, since a hot desk's monitors are all
known by their model and each would ask.

## Decision

- **A known monitor is switched on at once**, as the desktop was last used (`monitors_plugged`): a monitor
  that goes by a saved entry, its own or its model's. With the desktop extending, it is extended
  to where the entry puts it; mirroring, it mirrors. No popup.
- **Only the monitors that just came are switched on** (`UdevData::activate_only`). One left off earlier,
  by closing the popup or choosing Do nothing, stays off when another is plugged in.
- **The popup opens for** a monitor that is new, one whose entry asks, and any monitor while the primary is
  alone (`Mode::Off`): what a monitor plugged in should do then isn't known. It opens once if several
  came and any needs it, and then covers all the idle ones.
- **Each saved display has `ask`**, off by default and kept in `displays.toml`: tick it, and that monitor
  gets the popup each time. It is a checkbox on the Saved displays list, and `SetAsk(id, ask)` on the bus;
  placing the monitor again keeps it. A monitor that goes by its model's entry goes by that entry's `ask`.
- Ctrl+Super+P still opens the popup whenever it's wanted.

## Consequences

- A hot desk comes up in place with nothing to do, which was the point of remembering by model.
- A known monitor is lit without being asked, so one that shouldn't be (a projector in a meeting room) needs
  its entry's Ask ticked once.
- A monitor gets the desktop's mode, not a choice of its own: mirroring or primary-only is for all of them.
- A monitor that is switched on and shouldn't have been is one Ctrl+Super+P, Primary only, away.
