# patina-bar

The bar along the bottom (or top) of the screen, and the desktop's switchboard: its buttons and
icons open the launcher and the tray's menus, its tabs are the list of open windows, and its
config file holds the settings the whole shell shares (the skin and the wallpaper).

## On the desktop

Modules on the left, centre and right, as waybar lays them out (ADR-0001):

- `launcher`: the Sector F flask, which opens patina-launcher.
- `windows`: a tab per open window (ADR-0012, ADR-0027). Clicking one focuses it, middle-click
  closes it; the compositor's Alt+Tab and Super+1–9 follow the same order.
- `tray`: Wi-Fi (opens patina-wifi), the speaker (opens patina-volume) and the battery
  (ADR-0021, ADR-0022, ADR-0024).
- `clock`, and under dwl `tags`, `layout`, `title` and `appid`.
- `power`: the power button after the clock, which opens patina-power: Lock, Log Out, Restart
  and Shut Down (ADR-0048).

## How it fits in

- **Started by** patina-compositor at start-up, as a layer-shell client with an exclusive zone,
  so windows are placed above (or below) it. It also runs on other layer-shell compositors, and
  as dwl's status command (`dwl -s patina-bar`), reading dwl's status lines from stdin for the
  tags (`dwl.rs`, ADR-0004).
- **Learns about windows** through wlr-foreign-toplevel-management (patina-shell's
  `toplevels.rs`), and asks the compositor to activate or close them.
- **Opens the other pieces** by running their commands through `sh`, with `PATINA_SKIN` and
  `PATINA_BAR_EDGE` set so they match it and open from its edge: `launcher`, `volume`, `wifi` and
  `power` in its config. Under patina-compositor it asks the compositor to start them (`Spawn` on the
  session bus), so they're listed in Settings' Processes page with their output (ADR-0045);
  anywhere else, or without a session bus, it starts them itself.
- **Owns the shared config**, `~/.config/patina-bar/config.toml` (`config.rs`): `position`,
  `height`, `skin`, the modules of each side, `clock` (a strftime format), the four commands,
  `tags`, and `wallpaper`, which patina-wallpaper draws. patina-menu writes `skin` and
  `wallpaper`; the bar checks the file's time each second and restyles itself, and the other
  pieces read it when they open.
- **Skins** are Patina's, built in or in `~/.config/patina-bar/skins`, and through the bar they
  are the whole shell's.

## Code

`main.rs` (start-up, threads reading dwl, volume, Wi-Fi and the battery), `bar.rs` (the modules
and their layout), `tray.rs`, `launcher.rs` (the flask button), `dwl.rs`, `config.rs`. The
surface and drawing come from patina-shell.

## Running

```sh
make run-bar                      # on the current compositor (needs layer-shell, so not GNOME)
dwl -s patina-bar                 # on dwl, with its tags
patina-bar -c some-config.toml
```
