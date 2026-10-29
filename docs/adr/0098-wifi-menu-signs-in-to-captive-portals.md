# 0098. The Wi-Fi menu signs in to captive portals, by NetworkManager's connectivity check

Date: 2026-10-29

## Status

Accepted

## Context

Joining a café's open network succeeded and the menu said "Connected", but nothing worked until a
plain-HTTP page was opened and redirected to the portal's login. The menu never said so.
NetworkManager already knows: its connectivity check (where the distribution enables it, as Ubuntu
does) sets `Connectivity` to `PORTAL` when the check URL is answered with something else.

## Decision

- `Wifi::sign_in` is `Some(ConnectivityCheckUri)` while `Connectivity` is `PORTAL`. That address is
  plain HTTP, so the portal redirects it to its login page, as GNOME's sign-in popup relies on.
- The menu shows "This network needs you to sign in" with a Sign in button that runs `xdg-open`, and
  the active row says "Sign-in needed".
- While at a portal the worker calls `CheckConnectivity` every five seconds (`WifiBackend::recheck`),
  since NetworkManager would otherwise look again only every few minutes.
- Linux only: the wpa_supplicant backend reports no portal (ADR-0095), and a probe of our own is left
  for when FreeBSD needs it.

- The Linux VM fakes a portal (ADR-0096): `vm.sh fake portal on|off`. `vm-linux/fake-portal.py`
  answers NetworkManager's check (`patina-vm.conf` points it at `http://10.42.0.1/`) with a redirect
  to a page whose Accept button lets you through. To be reachable the fake access point (hostapd,
  dnsmasq and the portal) now runs in a network namespace, `patina-ap`: in one with wlan0, 10.42.0.1
  was the host's own address and wlan0-bound traffic to it went nowhere.

## Consequences

- Where NetworkManager's connectivity check is off, nothing is detected and the menu is as before.
- The bar's icon doesn't yet show it; the menu has to be opened.
