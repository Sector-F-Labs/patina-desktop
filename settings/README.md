# patina-settings

The desktop's Settings (ADR-0038): a Patina app window for the settings that need more than the
desktop's menu. Also builds `patina-night-light`, Night Light from the command line.

## On the desktop

A sidebar of pages, the page beside it, and a status bar for what an action did:

- **Appearance**: the shell's skin, as the desktop's menu offers it, with the skins folder
  buttons; and gaps (ADR-0051), on or off, their size and the floating bar's corner radius,
  written to `[gaps]` in the compositor's config.
- **Bar** (ADR-0050): what the window tabs show (icons and titles, icons, titles), which edge the
  bar is on, and the button that opens its config file for everything else; and its layout, one
  bar or split (ADR-0063), each part dragged to its spot on a map of the screen.
- **Wallpaper**: the default "Sector F Labs" and the images in `~/Pictures/Wallpapers` and the
  system's backgrounds, as thumbnails decoded off the UI thread.
- **Keyboard** (ADR-0068): the layouts Ctrl+Space steps through, moved, removed, switched to
  and added from xkb's by search, and key repeat, written to `[keyboard]` in the compositor's
  config; while it has none, the system's are shown, and the first change copies them in.
- **Night Light** (ADR-0039): off, always on, or on a schedule, and the warmth. It also shows
  what the compositor says: whether this screen can be warmed, whether it's on now (with a button
  to turn it off until the schedule changes, or back), and which apps are holding it off.

- **Power** (ADR-0042, ADR-0047): when the screen turns off without input (never, or 1 to 15
  minutes) and whether that locks it, written as `[screen] off_after` and `[lock] on_screen_off`
  in the compositor's config; and the power modes this machine has (performance, balanced, power
  saver) through power-profiles-daemon, why performance is held back, and which apps hold a mode.

- **Processes** (ADR-0045): what the compositor started, with each one's state, PID, uptime and
  restarts, and buttons to restart or stop it. A click on a name opens a window that follows its
  output. Settings can't restart itself.

It wears the shell's skin and follows it when the desktop's menu changes it.

## How it fits in

- **Started by** patina-menu: its Wallpaper… item runs `patina-settings --page wallpaper`, and
  Settings… runs it at Appearance. `--page appearance|bar|wallpaper|night-light|power|processes`.
- **Writes the files the settings live in**, keeping the rest of each: the bar's config
  (`skin`, `wallpaper`) through patina-shell, and `[night_light]` in the compositor's config with
  `toml_edit`. The bar, patina-wallpaper and the compositor watch those files and change at once.
- **Asks the compositor** how Night Light stands, and sets overrides, over D-Bus through
  `patina_system::night_light`. Nested in another desktop nothing serves it, and the page says so.
  The Processes page asks it the same way, through `patina_system::processes`.

## patina-night-light

```sh
patina-night-light                    # status: on/off, temperature, override, inhibitors
patina-night-light off                # off until the schedule next changes; on, auto
patina-night-light inhibit -- darktable   # held off while a command runs
```

## Code

`main.rs`, `app.rs` (the window, the Appearance and Wallpaper pages), `bar.rs` (the Bar page), `night_light.rs` and
`power.rs` and `processes.rs` (theirs),
`compositor_config.rs` (`[night_light]` read and written in place), `wallpapers.rs` (the images on
offer and their thumbnails), `bin/patina-night-light.rs`. `packaging/` holds its icon
(`settings.svg`, also drawn as the window icon) and its desktop entry, which `make install` puts
in `~/.local/share` (ADR-0062).

## Running

```sh
cargo run -p patina-settings -- --page night-light
PATINA_SETTINGS_PREVIEW=dir cargo test -p patina-settings preview -- --ignored   # each page as a PNG
```
