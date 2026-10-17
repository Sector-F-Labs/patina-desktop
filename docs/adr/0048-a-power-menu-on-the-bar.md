# 0048. A power menu at the bar's right end: lock, log out, restart and shut down

Date: 2026-10-17

## Status

Accepted

## Context

There was no way to end a Patina session or turn the machine off from the desktop, short of
Ctrl+Alt+Backspace or a terminal. The user asked for a power button to the right of the clock,
opening a menu with Lock, Log Out, Shut Down and Restart, as GNOME's system menu has.

Following ADR-0027, it grows where the tray's other menus live: a module on the bar, opening its
own small menu as patina-wifi and patina-volume do, at the same corner.

How each action is done:

- **Lock** could run `patina-lock` directly, but the compositor's `[lock] command` may be another
  locker; asking the compositor runs the one configured.
- **Log Out** is the compositor quitting cleanly: under patina-session (ADR-0046) that ends the
  session, and the login screen comes back. logind's TerminateSession was considered; quitting
  the compositor is what Ctrl+Alt+Backspace already does, and works the same without logind.
- **Restart and Shut Down** are the login manager's: logind's Reboot and PowerOff on the system
  bus, which ask polkit and, for the active local session, need no password. FreeBSD has no
  logind; ConsoleKit2 has the same methods, so it's tried after it. Where neither is there, or
  they say no, the items are greyed out.

## Decision

- **patina-power** (`power/`), a layer-shell menu like patina-wifi: Lock, Log Out…, Restart… and
  Shut Down…, at the bar's right end, in the bar's skin. The three that end the session ask
  first, in the menu ("Every app closes."), with Cancel; Escape takes the question back, then
  closes the menu. What can't be done here is greyed out, with why on hover. A failure (polkit
  refusing, say) is shown in the menu, which stays usable. One copy runs at a time; running it
  again closes it.
- **The bar** gains a `power` module, a power symbol (`patina_system::icons::power`), after the
  clock by default, running its `power` command (`patina-power`) through the compositor's
  `Spawn` (ADR-0045) like the tray's menus. A config with its own `right` list needs `power`
  added to show it.
- **The compositor** serves `com.sectorflabs.Patina.Session` at `/com/sectorflabs/Patina/Session`
  (`session/dbus.rs`): `Lock()`, which runs the `[lock] command`, and `LogOut()`, which quits
  cleanly. Nested, logging out closes the window.
- **patina-system's `session.rs`** is the client side: what's possible here (`abilities`), Lock
  and Log Out through the compositor, and Restart and Shut Down through logind or ConsoleKit2,
  interactive, so polkit may ask. Lock falls back to running `patina-lock` where the compositor
  can't be asked.

## Consequences

- Leaving the desktop and turning the machine off are a click and a confirmation away.
- Restart and Shut Down rely on the login manager letting the active session do it without a
  password; where polkit wants one (another user logged in, say), there is no polkit agent in a
  Patina session yet, so it fails, and the menu says so.
- The VM, without logind or ConsoleKit2, greys out Restart and Shut Down; only Lock and Log Out
  can be tried there.
- Suspend isn't offered yet; it would be a logind call beside these.
