# patina-settings

The desktop's Settings (ADR-0038): a Patina app window for the settings that need more than the
desktop's menu. Also builds `patina-night-light`, Night Light from the command line.

## On the desktop

A sidebar of pages, the page beside it, and a status bar for what an action did:

- **Appearance**: the shell's skin, as the desktop's menu offers it, with the skins folder
  buttons.
- **Wallpaper**: the default "Sector F Labs" and the images in `~/Pictures/Wallpapers` and the
  system's backgrounds, as thumbnails decoded off the UI thread.
- **Night Light** (ADR-0039): off, always on, or on a schedule, and the warmth. It also shows
  what the compositor says: whether this screen can be warmed, whether it's on now (with a button
  to turn it off until the schedule changes, or back), and which apps are holding it off.

It wears the shell's skin and follows it when the desktop's menu changes it.

## How it fits in

- **Started by** patina-menu: its Wallpaper… item runs `patina-settings --page wallpaper`, and
  Settings… runs it at Appearance. `--page appearance|wallpaper|night-light`.
- **Writes the files the settings live in**, keeping the rest of each: the bar's config
  (`skin`, `wallpaper`) through patina-shell, and `[night_light]` in the compositor's config with
  `toml_edit`. The bar, patina-wallpaper and the compositor watch those files and change at once.
- **Asks the compositor** how Night Light stands, and sets overrides, over D-Bus through
  `patina_system::night_light`. Nested in another desktop nothing serves it, and the page says so.

## patina-night-light

```sh
patina-night-light                    # status: on/off, temperature, override, inhibitors
patina-night-light off                # off until the schedule next changes; on, auto
patina-night-light inhibit -- darktable   # held off while a command runs
```

## Code

`main.rs`, `app.rs` (the window, the Appearance and Wallpaper pages), `night_light.rs` (its page),
`compositor_config.rs` (`[night_light]` read and written in place), `wallpapers.rs` (the images on
offer and their thumbnails), `bin/patina-night-light.rs`.

## Running

```sh
cargo run -p patina-settings -- --page night-light
PATINA_SETTINGS_PREVIEW=dir cargo test -p patina-settings preview -- --ignored   # each page as a PNG
```
