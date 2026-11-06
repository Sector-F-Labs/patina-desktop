# patina-hud

The bar along the bottom (or top) of the screen, and the desktop's switchboard: its buttons and
icons open the launcher and the tray's menus, its tabs are the list of open windows, and its
config file holds the settings the whole shell shares (the skin and the wallpaper).

## On the desktop

Modules on the left, centre and right, as waybar lays them out (ADR-0001):

- `launcher`: the Sector F flask, which opens patina-launcher.
- `windows`: a tab per open window (ADR-0012, ADR-0027), saying "App - Title" (ADR-0061). Clicking one focuses it, middle-click
  closes it; the compositor's Alt+Tab and Super+1–9 follow the same order. Each shows its app's
  icon, its title, or both, by `window_tabs` (ADR-0050); `patina_system::apps` matches the window's app id
  to its `.desktop` entry, and `icons.rs` finds the icon through the icon theme, on a thread.
  A tab's right-click menu opens in patina-menu, so it isn't cut off at the bar's edge
  (`menu.rs`, ADR-0056): New Window, which runs the entry's `new-window` action, or its Exec
  (ADR-0055), Pin to Bar or Unpin from Bar, and Close Window. Pinned apps come first (ADR-0080),
  `pinned` in the config: each as its windows' tabs, or, with none open, a faded icon that starts
  it. Dragging a tab moves it: a pinned app among the pins, a window onto the pins pins its app,
  and among the other windows it moves along them, which the compositor keeps
  (`com.sectorflabs.Patina.Tabs`), so its keys follow; `pins.rs` decides. Tooltips, the tabs' titles and the tray's, show in
  `patina-menu --tooltip` past the bar's edge too (`tooltip.rs`, ADR-0057); egui's own are off.
- `tray`: apps' status icons first (ADR-0064): the bar is the system tray apps such as Steam
  register with (`patina_system::sni`, StatusNotifierItem on the session bus); a click
  activates the app, a right click opens its menu in patina-menu. Then, with more than one
  keyboard layout, the one in use ("US"; a click lists them to switch, ADR-0068), Wi-Fi (opens patina-wifi), the speaker (opens patina-volume) and the battery
  (opens Settings' Power and Battery page; ADR-0021, ADR-0022, ADR-0024).
- `media` (ADR-0101): previous, play or pause and next for the media player playing (any MPRIS
  player: a browser tab, UnAmp), with the track on hover; nothing while none is. It's next to `tray`
  in the default `right`; leave it out of the lists to turn it off.
- `clock`, the date in full on hover, which opens patina-clock: a month and the day's events from
  Calendar (ADR-0082); and under dwl `tags`, `layout`, `title` and `appid`.
- `power`: the power button after the clock, which opens patina-power: Lock, Log Out, Restart
  and Shut Down (ADR-0048).

## How it fits in

- **Started by** patina-compositor at start-up, as a layer-shell client with an exclusive zone,
  so windows are placed above (or below) it. It also runs on other layer-shell compositors, and
  as dwl's status command (`dwl -s patina-hud`), reading dwl's status lines from stdin for the
  tags (`dwl.rs`, ADR-0004).
- **Learns about windows** through wlr-foreign-toplevel-management (patina-shell's
  `toplevels.rs`), and asks the compositor to activate or close them.
- **Opens the other pieces** by running their commands through `sh`, with `PATINA_SKIN` and
  `PATINA_HUD_EDGE` set so they match it and open from its edge: `launcher`, `volume`, `wifi`,
  `battery`, `calendar` and `power` in its config. Under patina-compositor it asks the compositor to start them (`Spawn` on the
  session bus), so they're listed in Settings' Processes page with their output (ADR-0045);
  anywhere else, or without a session bus, it starts them itself.
- **Owns the shared config**, `~/.config/patina-hud/config.toml` (`config.rs`): `position`,
  `height`, `skin`, the modules of each side, `clock` (a strftime format), the commands,
  `tags`, `pinned` (the pinned apps, which the compositor reads too), and `wallpaper` and
  `wallpaper_fit` (fill, fit or centre), which patina-wallpaper draws. patina-menu writes `skin` and
  `wallpaper`; the bar checks the file's time each second and restyles itself, and the other
  pieces read it when they open.
- **Floats with gaps** (ADR-0051): `gaps.rs` reads `[gaps]` from the compositor's config, which
  the bar checks each second too. With gaps on, the bar's layer-shell margin keeps it the gap
  away from the screen's edges, and it draws with rounded corners and an outline all round.
- **Skins** are Patina's, built in or in `~/.config/patina-hud/skins`, and through the bar they
  are the whole shell's.

## Elements, and hiding them

The HUD is the split layout (ADR-0063, the default, ADR-0110; `layout = "bar"` is the old single
strip, for dwl). Each element has `launcher_mode`, `windows_mode`, `tray_mode` and `media_mode`:
`always`, `autohide` (off the screen but for a 2 px sliver, which slides it in when the pointer
touches it, and keeps no space) or `hidden` (not started at all). **Super+H** runs `patina-hud
--toggle`, which makes or removes `$XDG_RUNTIME_DIR/patina-hud/hidden`; every process looks for
it a few times a second, and the elements that are `always` hide like `autohide` and give their
space back (`hide.rs`, ADR-0111).

With the split layout the launcher, the window list and the tray each sit on their
own, at `launcher_at`, `windows_at` and `tray_at`: an edge, then where along it (`bottom-left` is
a row, `left-bottom` a column). The media controls stay in the tray's part unless `media_at` (the
same names) gives them a part of their own (ADR-0101). This process draws the window list and runs the others as
`patina-hud --section`, which end when its pipe to them closes; each edge with a part on it is
kept by a pixel-thin `reserve-EDGE` section. `split.rs` places every surface and turns a point on
it into a place on the screen, which menus, tooltips and the pieces the bar opens
(`PATINA_OPEN_AT`) open at. Settings' Bar page places the parts by dragging.

## Code

`main.rs` (start-up, threads reading dwl, volume, Wi-Fi, the keyboard layout, the battery and the
compositor's window moves), `bar.rs` (the modules and their layout), `pins.rs` (what a dropped
tab does), `tray.rs`, `launcher.rs` (the flask button), `dwl.rs`, `config.rs`. The
surface and drawing come from patina-shell.

## Running

```sh
make run-bar                      # on the current compositor (needs layer-shell, so not GNOME)
dwl -s patina-hud                 # on dwl, with its tags
patina-hud -c some-config.toml
```
