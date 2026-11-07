# patina-displays

The popup for several monitors (ADR-0121). The compositor opens it on Ctrl+Super+P or the display
key (`displays` in `[keys]`) and when a monitor is plugged in: a pill at the bottom of the screen,
like the screenshot's, on the primary monitor, naming the monitors and offering **Extend** (each
shows its own part of one desktop), **Mirror** (each shows the same) or a third that does nothing.
The one in use is marked. A monitor plugged in stays off until it's chosen for; the third tile is
then **Do nothing**, which leaves it off, and otherwise **Primary only**, which turns the others
off.

## How it fits in

- **The choice goes through the bus**: `patina_system::displays` (the compositor's
  `com.sectorflabs.Patina.Displays`) lists the monitors and sets the mode. The compositor keeps the
  state, in `displays.toml` by monitor, so a monitor placed before comes back where it was.
- **A new monitor goes to Settings.** Extending with a monitor that has never been placed opens
  `patina-settings --page displays`, to choose which side of another it sits on.
- **A layer-shell overlay** on patina-shell, keyboard exclusive, closing on Escape or when the
  keyboard goes elsewhere.

How the compositor, this popup and Settings split the work, with diagrams, is in
[HOW-IT-WORKS.md](HOW-IT-WORKS.md).

## Code

`main.rs`, the surface and the choice; `hud.rs`, the pill.

## Running

```sh
cargo test -p patina-displays
patina-displays   # inside the desktop, on the hardware: it asks the compositor
```
