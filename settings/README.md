# patina-settings

The desktop's Settings (ADR-0038): a Patina app window for the settings that need more than the
desktop's menu. Also builds `patina-night-light`, Night Light from the command line.

## On the desktop

A sidebar of pages, the page beside it, and a status bar for what an action did:

- **Appearance**: the shell's skin, as the desktop's menu offers it, with the skins folder
  buttons; the style (`color_scheme.rs`, ADR-0087), Default, Light or Dark, for apps through
  GNOME's `color-scheme` setting; and gaps (ADR-0051), on or off, their size and the floating bar's corner radius,
  written to `[gaps]` in the compositor's config; whether the desktop draws the shadows Patina apps
  ask for (ADR-0112), written to `[shadows]`; and the icon theme (`icons.rs`), the installed
  ones offered, for Files and GTK apps through GNOME's `icon-theme` setting for now.
- **Bar** (ADR-0050): what the window tabs show (icons and titles, icons, titles), which edge the
  bar is on, whether its parts have shadows and how they look (`shadows` and `shadow_*` in its config, ADR-0113), and the button that opens its config file for everything else; and its layout, one
  bar or split (ADR-0063), each part dragged to its spot on a map of the screen.
- **Wallpaper**: the default "Sector F Labs" and the images in `~/Pictures/Wallpapers` and the
  system's backgrounds, as thumbnails decoded off the UI thread.
- **Account** (ADR-0104): your picture for the lock screen: the Sector F flask in each skin's
  colours, an image from `~/Pictures/Avatars`, or any picture through the desktop's file chooser
  (the portal, which is Files in the Patina session), cropped square and written to `~/.face`.
- **Defaults** (ADR-0079): the app that opens links and each kind of file Files opens (web
  browser, email, calendar, folders, images, documents, text, music, video, archives), chosen
  from the installed apps that take it and saved in `~/.config/patina/defaults.toml`, which
  `patina_system::defaults` writes out as `~/.config/patina-mimeapps.list` for xdg-open, read in
  Patina only; each row shows what opens it now.
- **Keyboard** (ADR-0068): the layouts Ctrl+Space steps through, four at most (any past the
  fourth shown as not used), moved, removed, switched to
  and added from xkb's by search, and key repeat, written to `[keyboard]` in the compositor's
  config; while it has none, the system's are shown, and the first change copies them in.
- **Dictation** (ADR-0073): the key held to dictate, read from `[keys]` in the compositor's
  config; and patina-dictate's config (`dictation.rs`, ADR-0119): the speech model, chosen from a
  short list and downloaded with curl (`dictation_models.rs`), the language, the accuracy
  settings, when the model is loaded and put away (at start, in a fullscreen window, when idle,
  ADR-0129), and whether the overlay shows at the bottom or top.
- **Clipboard** (ADR-0114, ADR-0116): whether the compositor keeps a history of what's copied,
  how many copies, for how long, and whether screenshots go on the clipboard, written to `[clipboard]` in the
  compositor's config (`clipboard.rs`); Clear history asks the compositor over the session bus.
- **Displays** (ADR-0121): the monitors drawn to scale (`displays.rs`), the others dragged to a
  side of one (`display_layout.rs` is the snapping, tested), or chosen from menus; extend, mirror
  or primary only; monitors connected but off, to extend or mirror onto; and the saved displays,
  grayed when none is connected, each with Ask when plugged in and Forget (ADR-0124, ADR-0126). It all goes through
  the compositor's `com.sectorflabs.Patina.Displays` (`patina_system::displays`), which keeps the
  places by monitor in `displays.toml`. The popup sends a monitor never placed here.
- **Night Light** (ADR-0039): off, always on, or on a schedule, and the warmth. It also shows
  what the compositor says: whether this screen can be warmed, whether it's on now (with a button
  to turn it off until the schedule changes, or back), and which apps are holding it off.

- **Power and Battery** (ADR-0042, ADR-0047, ADR-0081): when the screen turns off without input
  (never, or 1 to 15 minutes) and whether that locks it, written as `[screen] off_after` and
  `[lock] on_screen_off` in the compositor's config; the power modes this machine has
  (performance, balanced, power saver) through power-profiles-daemon, why performance is held
  back, and which apps hold a mode; and with a battery, its health and graphs of its charge and
  power draw over a day or a week, from UPower's history, painted from the skin's visuals.

- **Processes** (ADR-0045): what the compositor started, with each one's state, PID, uptime and
  restarts, and buttons to restart or stop it. A click on a name opens a window that follows its
  output. Settings can't restart itself.

It wears the shell's skin and follows it when the desktop's menu changes it.

## How it fits in

- **Started by** patina-menu: its Wallpaper… item runs `patina-settings --page wallpaper`, and
  Settings… runs it at Appearance; and by the bar's battery, at Power and Battery (its `battery` command). `--page appearance|bar|wallpaper|defaults|keyboard|dictation|clipboard|displays|night-light|power|processes`.
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

`main.rs`, `app.rs` (the window, the Appearance and Wallpaper pages), `account.rs` (the Account page), `bar.rs` (the Bar page),
`defaults.rs` (the Defaults page), `night_light.rs` and
`power.rs` and `processes.rs` (theirs), `battery.rs` (the Power and Battery page's battery and its
graphs; `cargo test -p patina-settings preview_the_battery -- --ignored` with
PATINA_SETTINGS_PREVIEW=dir draws them from made-up history),
`compositor_config.rs` (`[night_light]` read and written in place), `wallpapers.rs` (the images on
offer and their thumbnails), `bin/patina-night-light.rs`. `packaging/` holds its icon
(`settings.svg`, also drawn as the window icon) and its desktop entry, which `make install` puts
in `/usr/local/share` (ADR-0062).

## Running

```sh
cargo run -p patina-settings -- --page night-light
PATINA_SETTINGS_PREVIEW=dir cargo test -p patina-settings preview -- --ignored   # each page as a PNG
```
