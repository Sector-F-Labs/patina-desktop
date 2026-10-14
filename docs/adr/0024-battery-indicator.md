# 0024. A battery indicator in the tray, read from sysfs

Date: 2026-10-15

## Status

Accepted

## Context

On the laptop there was no way to see the battery. UPower offers it over D-Bus, with estimates;
`/sys/class/power_supply` has the same raw numbers (charge, status, and the energy and power to
estimate the time left) as plain files, on every Linux laptop, without a daemon.

## Decision

`patina_shell::power` reads the machine's batteries from sysfs (skipping devices' batteries such as
mice, which have `scope` Device), combining two into one: the charge, charging or not, and the time
to empty or to full at the current draw. The bar's tray shows, after the speaker, a battery filled
to the charge with a bolt while charging and the percentage beside it, in the error colour at 10% or
under while discharging, and the time left on hover. It's read every 5 s, so plugging in shows
soon; machines without a battery show nothing.

## Consequences

- No D-Bus or daemon for the battery; FreeBSD will need its own reading (acpiconf or sysctl).
- No warning beyond the colour, no power menu (suspend, power off) and no power profiles yet.
