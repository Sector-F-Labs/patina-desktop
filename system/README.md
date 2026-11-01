# patina-system

What the desktop's pieces read and change on the machine, and the drawings they share. It's the
desktop-specific half of what was patina-shell; the generic half (the layer-shell host and the
painter) is [patina-shell](https://github.com/Sector-F-Labs/patina-shell), in its own repository
(ADR-0037).

- **`audio.rs`**: the default output's volume and mute through PipeWire's `wpctl`, for the
  bar's speaker and patina-volume.
- **`network.rs`**: Wi-Fi, through a `WifiBackend` that `network::open` picks: NetworkManager on the
  system D-Bus (zbus), or `network/wpa.rs`, wpa_supplicant through wpa_cli, on FreeBSD (ADR-0095): devices, access
  points, saved connections, scanning, joining, the radio switch (ADR-0022). For the bar's
  Wi-Fi icon and patina-wifi.
- **`power.rs`**: the battery for the tray (ADR-0024), from a `PowerSource`: `power/sysfs.rs`
  (`/sys/class/power_supply`), or `power/acpi.rs` on FreeBSD (sysctl and acpiconf, ADR-0095).
- **`backlight.rs`**: the screen's brightness, read from sysfs and set through logind's
  SetBrightness without root (ADR-0020). The compositor's brightness keys and patina-volume's
  flash use it.
- **`icons.rs`**: the tray's small icons, drawn from shapes in a colour taken from the skin.
- **`keyboard.rs`**: the keyboard layouts through the compositor's
  `com.sectorflabs.Patina.Keyboard` (ADR-0068): which it has and which is in use, followed as they
  change, and switching; the layouts xkb offers, from its `evdev.xml` files, and each one's label
  ("US", "ᚠ"). For the bar's tray and Settings' Keyboard page.
- **`mpris.rs`**: the media players on the session bus (MPRIS, ADR-0101): which is playing, its
  track, and play/pause, next and previous, for the bar's media controls. `cargo run -p
  patina-system --example media` prints them.
- **`night_light.rs`**: Night Light's state, override and inhibitors through the compositor's
  `com.sectorflabs.Patina.NightLight` on the session bus (ADR-0039), for Settings,
  `patina-night-light` and apps wanting true colour.
- **`power_profiles.rs`**: power modes (power-saver, balanced, performance) through
  power-profiles-daemon on the system bus (ADR-0042), for Settings' Power page.
- **`processes.rs`**: the processes the compositor started, their logs, and restarting or stopping
  them, through its `com.sectorflabs.Patina.Processes` on the session bus (ADR-0045), for Settings'
  Processes page.
- **`apps.rs`**: installed apps from their `.desktop` entries (id, name, icon, Exec, New Window
  action) and a window's app by its app id, for the bar's tab icons and New Window and for
  patina-raise.
- **`pins.rs`**: the apps pinned to the bar (`pinned` in its config) and the tabs' order that
  follows from them, worked out the same way by the bar and the compositor; and moving a window
  along the tabs through the compositor's `com.sectorflabs.Patina.Tabs` (ADR-0080).
- **`windows.rs`**: the open windows and each one's process, and bringing one forward, from the
  compositor's `com.sectorflabs.Patina.Tabs` (ADR-0088), for patina-processes.
- **`tiling.rs`**: the compositor's `com.sectorflabs.Patina.Tiling` (ADR-0049, ADR-0053): drags and
  Super+arrows in, placing windows and the preview out, and holding the Tiler name, for
  patina-tiler and other tilers (`cargo run -p patina-system --example thirds`).
- **`sni.rs`**: the system tray (ADR-0064): serves `org.kde.StatusNotifierWatcher` (or follows
  another program's), reads apps' StatusNotifierItems and their dbusmenu menus, for patina-bar.
  `cargo run -p patina-system --example tray` prints what's registered and its menus;
  `dbus-run-session -- cargo test -p patina-system sni -- --ignored` tests it against a fake app.
- **`logo.rs`**: the Sector F flask (the bar's launcher button, the default wallpaper) and Ubuntu
  Mono, the wallpaper's lettering, rasterised as masks the skin tints.

The calls that wait on a program or D-Bus run off the UI thread. The Linux services
(NetworkManager, wpctl, logind) are optional: where they're missing, the pieces say so or leave
the item out.

```sh
cargo run -p patina-system --example wifi      # what NetworkManager or wpa_supplicant reports
cargo run -p patina-system --example battery
cargo run -p patina-system --example power     # the power modes; an argument switches mode
cargo run -p patina-system --example icons     # draws the tray's icons to a PNG
```
