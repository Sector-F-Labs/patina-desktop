# patina-wifi

The Wi-Fi menu in the bar's tray (ADR-0022): the networks in range, joining one, disconnecting,
and Wi-Fi on and off.

## On the desktop

From the bar's Wi-Fi icon, at the bar's right end: a header with an on/off switch, then the
networks in range, strongest first, with their signal and a padlock on secured ones. A click joins
a saved or open network; a new secured one asks for its password first; the one you're on offers
Disconnect. Escape, a click elsewhere or the icon again closes it.

## How it fits in

- **Started by** the bar's Wi-Fi icon (its `wifi` command), with the bar's skin and edge. A
  second run closes the open one (patina-shell's `Single`).
- **Talks to NetworkManager** on the system D-Bus (patina-system's `network.rs`, with zbus): the
  devices, access points and saved connections, scanning, activating and adding connections, and
  the radio switch. On Ubuntu, NetworkManager's polkit rules let the active session do this
  without a password prompt. The calls run off the UI thread.
- **Captive portals** (ADR-0098): NetworkManager's `Connectivity` says `PORTAL` when the network
  holds us at a sign-in page; `Wifi::sign_in` then carries its `ConnectivityCheckUri`, the menu
  shows a Sign in button (`xdg-open`), and the worker calls `CheckConnectivity` every few seconds
  so the line goes after signing in. Linux only for now.
- **The bar's icon** reads the same module for the signal it shows.
- NetworkManager on Linux, wpa_supplicant on FreeBSD (ADR-0095); without a Wi-Fi device the menu says "No Wi-Fi
  device".

## Code

`main.rs` (one copy, the D-Bus worker, the surface), `ui.rs` (the switch, the list, the
password box).

## Running

```sh
cargo run -p patina-wifi
cargo run -p patina-system --example wifi   # what NetworkManager or wpa_supplicant reports, as text
```

It opens at the bar's button that opened it, from `PATINA_OPEN_AT` (patina-shell's
`piece::opening`, ADR-0063), or on the bar's edge without it.
