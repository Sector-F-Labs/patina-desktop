# 0095. The battery and Wi-Fi on FreeBSD, behind a trait each

Date: 2026-10-29

## Status

Accepted: tested on loki, a ThinkPad on FreeBSD 15.1.

## Context

patina-system read the battery from `/sys/class/power_supply` (ADR-0024) and Wi-Fi from
NetworkManager on D-Bus (ADR-0022). FreeBSD has neither: on loki the bar showed no battery and the
Wi-Fi menu couldn't be opened. What it has is the ACPI battery's sysctls (`hw.acpi.battery.life`,
`.state`, `.time`, `hw.acpi.acline`) and `acpiconf -i 0` for the capacity and cycles, and
wpa_supplicant, driven by `wpa_cli` over a control socket that `wheel` may use.

The first try put `#[cfg(target_os = "freebsd")]` on every function. It worked, but the FreeBSD
code was only compiled, and its parsers only tested, on FreeBSD, and every caller sat on one side
of a cfg.

## Decision

- **One trait per thing, the implementation picked when asked.** `power::PowerSource` has
  `sysfs::Sysfs` and `acpi::Acpi`; `network::WifiBackend` has `NetworkManager` and
  `wpa::WpaSupplicant`. Every implementation builds everywhere, so each one's parsing is tested
  on any system, and nothing is `cfg`'d on the OS.
- **Power** picks sysfs where `/sys/class/power_supply` is a folder, else ACPI. `power::read` and
  `power::health` are as they were. ACPI reads by running `sysctl` and `acpiconf`, at the bar's
  pace. Charging time isn't known there (`minutes` is `None`); discharging time is ACPI's own
  estimate.
- **Wi-Fi** is `network::open()`, which returns NetworkManager if it owns its name on the system
  bus, else wpa_supplicant. The bar and the menu call that, not a type. `Network::access_point` is
  a `String` (NetworkManager's path, or the BSSID) so the type doesn't belong to either.
- **wpa_supplicant** lists networks from `scan_results`, the saved ones from `list_networks`, the
  active one from `status`, and joins with `select_network`, or `add_network`, `set_network`
  (the name in hex, the password quoted) and `save_config`. Wi-Fi off and on is the interface
  down and up, through `doas -n ifconfig`.

## Consequences

- Battery and Wi-Fi work on FreeBSD, and Linux behaves as before: one more D-Bus call
  (`NameHasOwner`) when the Wi-Fi opens.
- Saving a joined network needs `update_config=1` in `/etc/wpa_supplicant.conf`; without it the
  network works until the supplicant restarts. Patina OS's setup adds it.
- A password goes to `wpa_cli` as an argument, so it's visible in `ps` for a moment. A control
  socket client would avoid that.
- Wi-Fi off and on needs doas without a password, which is a choice for the machine, not Patina.
- No battery history (it comes from UPower), and enterprise and WEP networks can't be joined
  from the menu, as with NetworkManager.
- The same two traits are where backlight, volume and the rest of FreeBSD's differences can go,
  instead of cfgs.
