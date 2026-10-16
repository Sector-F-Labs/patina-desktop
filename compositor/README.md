# patina-compositor

The heart of the desktop: a floating Wayland compositor built on
[Smithay](https://github.com/Smithay/smithay). It owns the screen, the keyboard and the pointer,
places and draws windows, and starts the rest of the desktop as its clients. Every other piece in
this workspace is a Wayland client of it.

## On the desktop

- **Windows float** (ADR-0007): each is placed once, cascading in the area the bar leaves free; a
  click focuses and raises it. Super+drag moves any window; a client's own header bar moves and
  resizes it.
- **Placing windows from the keyboard** (ADR-0014, ADR-0023): Super+Up/Down maximise and
  restore, Super+Left/Right tile half-screen, Super+Shift+arrows grow and shrink, dragging a
  placed window restores it under the pointer.
- **Moving between windows** follows the bar's tabs (ADR-0027): Alt+Tab steps along them,
  Super+1–9 picks one, closing a window focuses its neighbouring tab. Super+W closes a window,
  Super+Q quits the app.
- **The desktop's own keys:** Super alone opens the launcher; Ctrl+Super+Q and Super+Escape lock
  (ADR-0026); Super+Shift+3 or Print screenshots the desktop and Super+Shift+4 the focused window
  (ADR-0034); the brightness and volume keys change them and flash the volume menu (ADR-0020,
  ADR-0033); Super+Space switches keyboard layout; Super+Shift+R reloads the config.
- **A right click on the empty desktop** opens patina-menu at the pointer (ADR-0029).
- **Three-finger drag** on a touchpad, on by default (ADR-0031); Patina's own cursors at every
  scale (ADR-0032); fractional scaling (ADR-0018).

## How it fits in

- **Two ways to run.** Nested, as a window on another desktop (GNOME, sway, the FreeBSD VM's
  dwl), drawn with OpenGL ES through winit (`winit.rs`, ADR-0015) or in software with pixman
  (`nested.rs`, `PATINA_RENDERER=pixman`, ADR-0008). Or on the hardware, as the login session or
  from a text console: DRM/KMS, GBM, GLES, libinput, and devices from logind through libseat
  (`udev.rs`, ADR-0016). Which one is decided by whether `WAYLAND_DISPLAY` is set when it starts.
- **It starts the desktop.** At start-up it runs `patina-wallpaper` and `patina-bar`, then any
  commands given on its command line. Its own keys run `patina-launcher`, `patina-menu --at X,Y`,
  `patina-lock` and `patina-volume --flash`/`--brightness` (all configurable). Clients get
  `WAYLAND_DISPLAY`, `XDG_CURRENT_DESKTOP=Patina`, and a `PATH` with the compositor's own folder
  first, so the pieces built beside it are found before installed copies (`clients.rs`).
- **It tells the session about itself.** On the hardware (or with `--session`) it exports
  `WAYLAND_DISPLAY` and `XDG_CURRENT_DESKTOP` to systemd and D-Bus, so services started later,
  such as xdg-desktop-portal and Files' file chooser, reach it.
- **The protocols the other pieces rely on:** xdg-shell for apps; wlr-layer-shell for the bar,
  menus and wallpaper; wlr-foreign-toplevel-management for the bar's window tabs (ADR-0012);
  ext-session-lock for patina-lock (ADR-0025); cursor-shape; wp_fractional_scale and
  wp_viewporter; linux-dmabuf with the GL backends; data-device for copy and paste.
- **Config:** `~/.config/patina-compositor/config.toml` (ADR-0019): scale, `[keyboard]`,
  `[touchpad]`, `[cursor]`, `[media]` (the volume commands, `wpctl` by default), `[lock]`, the
  launcher and desktop menu commands, and `[bindings]` from key combinations to commands. Every
  key is optional; a broken file falls back to the defaults and never stops the session.
- **Shares code with the shell pieces** through patina-system: the backlight (`backlight.rs`) for
  the brightness keys.

## Code

`main.rs` (start-up, choosing the backend), `state.rs` (the protocols, placement, focus),
`input.rs` and `keyboard.rs` (input and the compositor's own keys), `grabs.rs`, `maximize.rs`,
`tabs.rs` (moving and placing windows), `toplevels.rs`, `lock.rs`, `media.rs`, `screenshot.rs`,
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
