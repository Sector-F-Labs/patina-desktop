# 0042. Power modes in Settings, through power-profiles-daemon

Date: 2026-10-17

## Status

Accepted

## Context

The user wanted GNOME's power modes (Performance, Balanced, Power Saver) in Patina's Settings
(ADR-0038). GNOME doesn't switch them itself. power-profiles-daemon, a system service that Ubuntu
installs with GNOME, owns them, and GNOME's settings and top-bar menu are D-Bus clients of it
(`org.freedesktop.UPower.PowerProfiles`, formerly `net.hadess.PowerProfiles`). The daemon knows
what each machine's driver can do: `intel_pstate` and `amd_pstate` offer all three, ACPI
platform profiles add more, and a machine with neither gets a placeholder with Power Saver and
Balanced only, as this desktop has. Its polkit policy lets the person at an active local session
switch without a password.

Writing CPU governors or EPP hints ourselves was ruled out. It needs root, differs per driver,
and would fight the daemon wherever it's installed. A quick switch in the bar's tray, where GNOME
has one, was left out for now at the user's choice.

## Decision

- `patina_system::power_profiles` reads the daemon's profiles, the active one, why performance is
  degraded and which apps hold a mode, and sets the active profile. It uses the current name,
  then the old one.
- Settings gets a **Power** page: a radio button per profile the machine has, fastest first,
  with GNOME's descriptions; a note when there's no performance mode; the degraded reason (on a
  lap, too hot); and the holds ("Steam is holding Performance"). It asks the daemon every two
  seconds on a thread, so a change made elsewhere shows. Switching runs on a thread, and a
  failure goes to the status bar.
- Without the daemon (FreeBSD, or a system without it) the page says what to install.

## Consequences

- Power modes work on any Linux with power-profiles-daemon, with whatever modes the hardware has,
  and stay in step with anything else that switches them.
- FreeBSD has no power modes in Patina. A backend for `powerd`/`powerdxx` would be a new
  decision.
- No automatic power saving on low battery yet. GNOME does that through the daemon's holds, and
  it would live in the compositor or the bar.
- Changing mode is two clicks away, in Settings, rather than in the tray.
