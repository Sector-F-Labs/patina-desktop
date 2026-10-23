# 0081. The battery's history is graphed in Settings, from what UPower keeps

Date: 2026-10-23

## Status

Accepted. The user asked for clicking the bar's battery to open Settings at the battery, and then
for graphs of the battery's use drawn in egui, on the Power page, renamed Power and Battery. Where
the history comes from, and what is graphed, is my design.

## Context

The bar reads the battery from sysfs every few seconds ([ADR-0024](0024-battery-indicator.md)),
but only as it is now; nothing is kept. A graph needs the past. Two sources were considered:

- **UPower's history.** UPower records the charge, the power draw and the time left as they
  change, kept for days in `/var/lib/upower`, and serves it as `GetHistory` on each device over
  the system bus. It's already running on Ubuntu, so a new install has days of history at once.
- **A log of our own.** The bar could write a reading a minute somewhere under
  `$XDG_STATE_HOME`. That would work without UPower, but the graph would start empty, and the bar
  would be writing to disk all day for a page opened now and then.

FreeBSD would be the case for our own log, but the battery isn't read there at all yet (ADR-0024's
sysfs is Linux's).

## Decision

- **UPower's history, read when Settings is open.** `patina_system::battery_history` finds the
  battery that powers the machine (`Type` 2, `PowerSupply`) and asks for the charge and the rate
  over the last day or week, once a minute while the page is open. Without UPower the page says
  so; without a battery there's no battery section at all.
- **Health from sysfs**, beside the battery's reading: `energy_full` against `energy_full_design`
  (or the `charge_` pair) and `cycle_count`, so it shows without UPower too.
- **Painted by hand, not with `egui_plot`.** Two graphs, the charge and the power draw, over the
  last 24 hours or 7 days, shaded where it was plugged in, with the value under the pointer. They
  take every colour from `ui.visuals()`, so they follow the skin as the rest of Settings does,
  and they need no new dependency beyond chrono for local times.
- **Under the power settings**, on the same page: the page is Power and Battery, still
  `--page power`, and the bar's battery opens it (its `battery` command).

## Consequences

- The graphs are as good as UPower's record: it saves on change, so long gaps (suspended, or
  idle on the charger) draw as straight lines.
- No history on FreeBSD, nor on a Linux without UPower, though the health still shows.
- `cargo run -p patina-system --example battery` prints what the page reads.
