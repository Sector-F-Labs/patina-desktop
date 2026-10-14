# 0022. A Wi-Fi menu in the tray, talking to NetworkManager over D-Bus

Date: 2026-10-15

## Status

Accepted

## Context

On the laptop there was no way to pick a Wi-Fi network from the desktop: GNOME's network menu is
GNOME Shell's, nm-applet needs a tray that hosts other apps' icons (StatusNotifierItem), and nmtui
needs a terminal. The user asked for a tray Wi-Fi menu after trying the alternatives. Ubuntu's
networking is NetworkManager, whose D-Bus API lists devices, access points and saved connections and
can scan, connect and disconnect; its polkit rules let the active session do all of that, and let
members of `sudo` save new networks, without a prompt (there's no polkit agent yet).

## Decision

- **patina-wifi**, a shell piece like the volume menu (ADR-0021): a header with an on/off switch,
  then the networks in range (the strongest access point of each name), the connected one first,
  then saved ones, then by strength, each with its signal, a lock if secured, and Connected or Saved.
  A click joins a saved or open network; a new WPA/WPA3-personal network asks for its password in
  the menu; the connected one offers Disconnect. Enterprise (802.1X) and WEP networks are refused
  with a pointer to nmtui. Joining shows progress, and "Couldn't join" after 25 s (a wrong password).
- `patina_shell::network`: NetworkManager over the system bus with zbus, off the UI thread. A saved
  connection is activated by its path; a new one is made with AddAndActivateConnection, with
  `wpa-psk` or `sae` and the password.
- **The bar's tray** shows a Wi-Fi icon before the speaker: the signal in three arcs, or a slash
  when off or not connected, read every 3 s; a click runs `wifi = "patina-wifi"`.
- What the launcher, volume menu and Wi-Fi menu share (one copy per display with a socket, the bar's
  skin and edge) moves into `patina_shell::piece`.

## Consequences

- Wi-Fi works from the desktop wherever NetworkManager runs: Ubuntu, Fedora, most Linux desktops.
- New networks are saved system-wide, as GNOME's are by default; outside `sudo` or `netdev`, saving
  one would need a polkit agent.
- No hidden networks, enterprise logins or VPNs; nmtui or nm-connection-editor cover those for now.
