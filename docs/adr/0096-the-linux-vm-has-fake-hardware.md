# 0096. The Linux test VM has a fake battery and Wi-Fi, made by the kernel

Date: 2026-10-29

## Status

Accepted

## Context

Adding FreeBSD's battery and Wi-Fi (ADR-0095) meant changing the code Linux uses for both, and the
Linux VM (ADR-0017) couldn't tell us it still worked: it has no battery and no Wi-Fi device, so the
bar showed neither and the Wi-Fi menu said "No Wi-Fi device". Only a real laptop could check, which
is how it was checked.

## Decision

The VM sets up hardware with the kernel's own simulators, so the real code reads it
(`vm-linux/fake-hardware.sh`, run by `vm.sh` at every start and restart):

- **A battery and charger** from `test_power`: `/sys/class/power_supply/test_battery`, which
  `power/sysfs.rs` reads as it does a laptop's. `vm.sh fake battery 15 discharging` and
  `vm.sh fake ac off` change it, through the module's parameters.
- **Wi-Fi** from `mac80211_hwsim`: two radios, `wlan0` for NetworkManager (installed in the guest,
  managing only Wi-Fi), and `wlan1` as an access point under hostapd, `patina-test` with the
  password `patina-test-pass`, with a DHCP server (dnsmasq) so joining finishes, which
  NetworkManager scans and can join.

Both are loaded at every boot, so the VM has them whenever it's up.

## Consequences

- The VM's bar always shows a battery and a Wi-Fi icon, and the Wi-Fi menu has a network to join:
  the code BSD support touches runs on Linux, in a VM, on every start.
- polkit lets NetworkManager's scans and joins through for the active desktop session, not for
  an ssh shell, so from outside the VM Wi-Fi is driven through the desktop (the Wi-Fi menu), not
  `nmcli`.
- `test_power` reports `charge_now` out of 100 and a small `current_now`, so the time left it
  gives is not a real battery's; the charge, the status and the charger are.
- The first start after this installs NetworkManager, wpasupplicant and hostapd in the guest.
- Regression tests for this (ADR-0097) set the battery and join the access point, then
  check the bar and the menu.
