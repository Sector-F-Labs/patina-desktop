# Patina desktop

A cargo workspace (ADR-0009): `compositor/` is a floating Smithay compositor (ADR-0006, 0007),
`bar/` is a layer-shell egui status bar skinned with Patina, and the menus, lock screen and
wallpaper beside it; `system/` (patina-system) is what they read and draw from the machine. Two
parts live in public repositories of their own, by git (ADR-0037): **patina-shell**
(github.com/Sector-F-Labs/patina-shell), the layer-shell host and software painter every piece
is built on (ADR-0011), and **patina-launcher** (github.com/Sector-F-Labs/patina-launcher), the
launcher the bar's Sector F button opens, which `make install` builds from there. The compositor
puts its own folder first on its clients' PATH, so the pieces are found beside it.

To change patina-shell along with the desktop, clone it beside this repo and add
`[patch."https://github.com/Sector-F-Labs/patina-shell"] patina-shell = { path = "../patina-shell" }`
to a local, uncommitted `.cargo/config.toml`; push it, then `cargo update -p patina-shell` here.

Patina comes from its `develop` branch on GitHub (ADR-0013): change Patina there, push
`develop`, then `cargo update -p patina-ui` here. Never publish Patina without asking.

## Commands

- `make` lists targets; `make check` (workspace tests + clippy -D warnings) before committing.
- `make run` opens the compositor as a window on the current desktop (it pops up on the user's
  GNOME session, so prefer the VM).
- Test ground is a FreeBSD VM from `bsdt.toml`: dwl from packages is only the parent desktop;
  `gui.start` runs `target/release/patina-compositor` in it. `make vm`, then `make vm-restart`
  after changes, `make screenshot` (grim → docs/screenshot.png) and read the PNG back.
- The user often uses the VM live over VNC. Don't `bsdt type`/`bsdt key` into it without asking;
  grim screenshots are fine.
- Logs in the guest: /tmp/bsdt-gui.log (compositor, bar and the apps they start), /tmp/bsdt-session.log (dwl).

## Layout

Each crate has a README saying its role on the desktop and how it fits with the others; keep it
current when that changes.

- `compositor/src/state.rs`: server side: protocols, placement, focus, input into the seat.
- `compositor/src/drag.rs`: three-finger drag (ADR-0031): libinput swipes hold BTN_LEFT, with a
  grace timer on `State::loop_handle` between strokes.
- `compositor/src/screenshot.rs`: Super+Shift+3/4 and Print (ADR-0034): the desktop or the focused
  window drawn offscreen with the backend's renderer, saved as a PNG on a thread; then a `Flash`
  each backend draws on top (`flash_element`, `overlay`).
- `compositor/src/night_light.rs`: Night Light (ADR-0039): the colour maths, the `Schedule` trait,
  fades, overrides and inhibitors; `UdevData::set_temperature` sets the CRTC's gamma ramp (hardware
  only). `night_light/dbus.rs` serves `com.sectorflabs.Patina.NightLight`; `dbus-run-session --
  cargo test -p patina-compositor night_light -- --ignored` tests it on a bus of its own. The
  compositor reloads its config within a second of a save.
- `compositor/src/xwayland.rs`: X11 apps (ADR-0040): Xwayland spawned at start-up, `DISPLAY` for
  clients, and the `XwmHandler` that maps, places, moves and resizes X windows as `Window`s in
  the space. `focus.rs`: the keyboard focus is a `FocusTarget` (Wayland surface or X11 window, so
  the X input focus follows). `window_for` finds both kinds by their `wl_surface`.
- `compositor/src/fullscreen.rs`: fullscreen (ADR-0041): `State::fullscreen`, sized to the output,
  drawn over the layers by every backend and first under the pointer while it has the keyboard.
- `compositor/src/dnd.rs`: drag and drop between clients is Smithay's; this draws the icon a
  client drags (`State::dnd_icon`) at the pointer in every backend, moved by its attach offsets;
  `action_choice` in state.rs makes a drop a copy with Ctrl, a move with Shift.
- `compositor/src/cursor.rs`: Patina's cursors (ADR-0032), SVG shapes rendered per scale; served by
  name through cursor-shape. `State::grab_cursor` overrides the client's during moves and resizes.
  `cargo test -p patina-compositor cursor -- --ignored` with PATINA_CURSOR_PREVIEW=file.png draws
  them all.
- `compositor/src/winit.rs`: the window on the parent desktop with OpenGL ES (Smithay's winit
  backend) and linux-dmabuf (ADR-0015); tried first.
- `compositor/src/nested.rs`: the fallback (no EGL, or `PATINA_RENDERER=pixman`): an sctk window,
  pixman rendering copied into wl_shm, parent input → seat (ADR-0008). `State::nested` and
  `State::winit` are Options; exactly one is set. sctk traits are aliased (`Client*`) where their
  names clash with Smithay's.
- patina-shell (its own repo): `host.rs`, the layer-shell egui host (sctk), generic over a
  `Shell`, with `Options` for anchor, size, keyboard, per-output surfaces, lock and the window
  fallback where there's no layer shell; `raster.rs`, the CPU painter (ADR-0002); `piece.rs`, one
  copy per display over a socket; `bar.rs`, the bar's skin, edge and config; `toplevels.rs`.
- `system/`: patina-system: `audio.rs` (wpctl), `backlight.rs` (sysfs and logind), `network.rs`
  (NetworkManager with zbus; `cargo run -p patina-system --example wifi` prints what it reads),
  `night_light.rs` (the compositor's Night Light over D-Bus), `power_profiles.rs` (power modes), `power.rs` (battery), `icons.rs`
  (tray icons), `logo.rs` (the flask and Ubuntu Mono).
- `compositor/src/toplevels.rs` and patina-shell's `toplevels.rs`: wlr-foreign-toplevel-management,
  server and client, for the bar's `windows` tabs (ADR-0012).
- `wifi/`: patina-wifi, the tray's Wi-Fi menu (ADR-0022), on `system/src/network.rs`.
- `volume/`: patina-volume, the tray's volume menu, also flashed by the compositor after a
  volume key (`--flash`) or a brightness key (`--brightness`) (ADR-0021, ADR-0033).
  On `system/src/audio.rs` and `backlight.rs`.
- `lock/`: patina-lock, the lock screen (ADR-0028): `Options::lock` makes the host draw on
  ext-session-lock surfaces and unlock when the shell finishes; `pam.rs` declares the PAM calls.
  The compositor's side is `compositor/src/lock.rs` (ADR-0025).
- `menu/`: patina-menu, the desktop's menu on a right click on the desktop (ADR-0029): the skin
  menu, writing `skin` into the bar's config (patina-shell's `bar.rs` `set_skin`); the bar polls the
  file's mtime each second and restyles (`Host::restyle`). Wallpaper… opens Settings at its page.
- `settings/`: patina-settings (ADR-0038), an eframe Patina app: Appearance, Wallpaper (sets
  `wallpaper`) and Night Light (writes `[night_light]` into the compositor's config with
  `toml_edit`; state over D-Bus through `system/src/night_light.rs`) and Power
  (power-profiles-daemon through `system/src/power_profiles.rs`). Also `patina-night-light`.
  `cargo test -p patina-settings preview -- --ignored` with PATINA_SETTINGS_PREVIEW=dir draws
  each page in software, without opening a window.
- `wallpaper/`: patina-wallpaper (ADR-0030), a click-through background-layer surface: the image,
  or the default flask (`system/src/logo.rs`, with Ubuntu Mono) in the skin's colours. `cargo test
  -p patina-wallpaper -- --ignored` with PATINA_WALLPAPER_PREVIEW=dir renders the default per skin.
- `bar/src/dwl.rs`: dwl's status lines; `bar/src/bar.rs`: modules; `bar/src/config.rs`.

## Principle

Don't repeat the UI (ADR-0027): a feature grows where it already lives. The bar's tabs are the
window list (Alt+Tab steps along them, closing focuses the neighbouring tab); there's no separate
switcher.

## Gotchas

- `bsdt exec -- cmd ~/x` expands `~` on the host; use `bsdt exec -- sh -c '... $HOME/x'`.
- dwl blocks if a `-s` command stops reading stdin, so the bar reads stdin on its own thread.
- Paint colours only from `ui.visuals()`, so skins reach them.
- In egui, item_spacing changed inside a row leaks to the next widget; change it in `ui.scope`.
- sctk's `delegate_dispatch2!` blanket impl means extra client objects need a local user-data
  type implementing `Dispatch2`, not `()`.
