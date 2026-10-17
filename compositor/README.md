# patina-compositor

The heart of the desktop: a floating Wayland compositor built on
[Smithay](https://github.com/Smithay/smithay). It owns the screen, the keyboard and the pointer,
places and draws windows, and starts the rest of the desktop as its clients. Every other piece in
this workspace is a Wayland client of it.

## On the desktop

- **Windows float** (ADR-0007): each is placed once, cascading in the area the bar leaves free; a
  click focuses and raises it. Super+drag moves any window; a client's own header bar moves and
  resizes it.
- **Tiling's mechanism, not its policy** (ADR-0049, ADR-0053): the compositor reports drags and
  Super+arrows on the session bus and places windows and shows the preview where it's told;
  patina-tiler, which it starts and keeps running, decides (or a tiler that takes the
  `com.sectorflabs.Patina.Tiler` name from it). Dragging a placed window restores it under the
  pointer; Super+Shift+arrows grow and shrink it here. Only an app's own maximise button
  maximises (ADR-0052).
- **Moving between windows** follows the bar's tabs (ADR-0027): Alt+Tab steps along them,
  Super+1–9 picks one, closing a window focuses its neighbouring tab. Super+W closes a window,
  Super+Q quits the app.
- **The desktop's own keys:** Super alone opens the launcher; Ctrl+Super+Q and Super+Escape lock
  (ADR-0026); Super+Shift+3 or Print screenshots the desktop and Super+Shift+4 the focused window
  (ADR-0034); the brightness and volume keys change them and flash the volume menu (ADR-0020,
  ADR-0033); Super+Space switches keyboard layout; Super+Shift+R reloads the config.
- **A right click on the empty desktop** opens patina-menu at the pointer (ADR-0029).
- **X11 apps** (Steam, Discord, Wine) run through Xwayland (ADR-0040), started with the
  compositor; their windows float, focus, tab and move like any other.
- **Fullscreen** for games and videos that ask (ADR-0041): the whole screen, over the bar.
  Smithay comes from a fork whose X11 window manager names the focused window as the active one,
  so Wine's fullscreen games don't minimise themselves (ADR-0044).
- **Mouse-look** for games (ADR-0043): relative pointer motion, and the pointer locked or confined
  to a window that asks, on the hardware.
- **The screen turns off** after `[screen] off_after` seconds without input, five minutes by
  default (ADR-0047): DPMS off on the hardware, back on at any input. Video players (idle-inhibit)
  and Steam's games (`org.freedesktop.ScreenSaver`, which the compositor serves) hold it on.
- **Night Light** on the hardware (ADR-0039): the CRTC's gamma ramp warmed in the evening, by
  `[night_light]`'s mode and schedule, fading in and out. Apps and scripts can hold it off over
  D-Bus; nested, the parent desktop's own night light applies.
- **Three-finger drag** on a touchpad, on by default (ADR-0031); Patina's own cursors at every
  scale (ADR-0032); fractional scaling (ADR-0018).

## How it fits in

- **Two ways to run.** Nested, as a window on another desktop (GNOME, sway, the FreeBSD VM's
  dwl), drawn with OpenGL ES through winit (`winit.rs`, ADR-0015) or in software with pixman
  (`nested.rs`, `PATINA_RENDERER=pixman`, ADR-0008). Or on the hardware, as the login session or
  from a text console: DRM/KMS, GBM, GLES, libinput, and devices from logind through libseat
  (`udev.rs`, ADR-0016). Which one is decided by whether `WAYLAND_DISPLAY` is set when it starts.
- **It starts the desktop.** At start-up it runs Xwayland (when installed), `patina-wallpaper`,
  `patina-bar` and `patina-tiler`, then any commands given on its command line. Its own keys run
  `patina-launcher`, `patina-menu --at X,Y`, `patina-lock` and `patina-volume
  --flash`/`--brightness` (all configurable). Clients get `WAYLAND_DISPLAY`, `DISPLAY`
  (Xwayland's), `XDG_CURRENT_DESKTOP=Patina`, and a `PATH` with the compositor's own folder
  first, so the pieces built beside it are found before installed copies (`clients.rs`).
- **It keeps what it starts** (`processes.rs`, ADR-0045). The wallpaper, the bar and the tiler are
  started again when they exit, until one exits five times in thirty seconds. Every child's output is
  kept (its last 500 lines) and copied to the compositor's stderr with the child's name in front.
  The list is served as `com.sectorflabs.Patina.Processes` on the session bus, nested too, for
  Settings' Processes page to restart, stop and read them. `dbus-run-session -- cargo test -p
  patina-compositor processes -- --ignored` tests it on a bus of its own.
- **It restarts under patina-session** (`session.rs`, ADR-0046): Ctrl+Alt+Shift+Backspace, or
  `RestartDesktop` on the session bus, quits with status 75 for patina-session to start it again.
  It starts locked if the compositor before it left the screen locked.
- **It tells the session about itself.** On the hardware (or with `--session`) it exports
  `WAYLAND_DISPLAY`, `DISPLAY` and `XDG_CURRENT_DESKTOP` to systemd and D-Bus, so services started later,
  such as xdg-desktop-portal and Files' file chooser, reach it.
- **The protocols the other pieces rely on:** xdg-shell for apps; wlr-layer-shell for the bar,
  menus and wallpaper; wlr-foreign-toplevel-management for the bar's window tabs (ADR-0012);
  ext-session-lock for patina-lock (ADR-0025); cursor-shape; wp_fractional_scale and
  wp_viewporter; linux-dmabuf with the GL backends; data-device for copy and paste.
- **Config:** `~/.config/patina-compositor/config.toml` (ADR-0019): scale, `[keyboard]`,
  `[touchpad]`, `[cursor]`, `[media]` (the volume commands, `wpctl` by default), `[lock]`,
  `[night_light]`, `[gaps]` (space around tiled windows, ADR-0051), the launcher and desktop
  menu commands, and `[bindings]` from key combinations
  to commands. Every key is optional; a broken file falls back to the defaults and never stops the
  session. It's reloaded within a second of being saved (ADR-0039), so patina-settings only
  writes it.
- **On the session bus:** `org.freedesktop.ScreenSaver` (`idle/dbus.rs`), for apps holding the
  screen on; `com.sectorflabs.Patina.Session` (`session/dbus.rs`), Lock and LogOut for
  patina-power (ADR-0048); `com.sectorflabs.Patina.Tiling` (`tiling/dbus.rs`, ADR-0049), drags,
  windows and placing them, for tilers. On the hardware: `com.sectorflabs.Patina.NightLight` (Inhibit,
  Uninhibit, SetOverride, and its state as properties), for patina-settings,
  `patina-night-light` and apps that need true colour.
- **Shares code with the shell pieces** through patina-system: the backlight (`backlight.rs`) for
  the brightness keys.

## Code

`main.rs` (start-up, choosing the backend), `state.rs` (the protocols, placement, focus),
`input.rs` and `keyboard.rs` (input and the compositor's own keys), `grabs.rs`, `maximize.rs`,
`tiling.rs` with `tiling/dbus.rs`, `tabs.rs` (moving and placing windows), `toplevels.rs`, `lock.rs`, `media.rs`, `screenshot.rs`,
`night_light.rs` with `night_light/dbus.rs`, `idle.rs` with `idle/dbus.rs`, `xwayland.rs` and `focus.rs` (X11 windows and
what has the keyboard), `fullscreen.rs`, `constraints.rs` (pointer locks for games), `dnd.rs` (the icon a drag shows),
`cursor.rs`, `drag.rs`, `scale.rs`, `config.rs`, `clients.rs`, and the backends `winit.rs`,
`nested.rs`, `udev.rs` with `dmabuf.rs`.

## Running

```sh
make run                          # nested on this desktop, with the bar
patina-compositor foot            # and a terminal
patina-compositor --session       # as the login session (make install-session adds it to GDM)
```

See [docs/running-as-a-session.md](../docs/running-as-a-session.md) for running it as your
desktop.
