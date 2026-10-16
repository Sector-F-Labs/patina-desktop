# Patina desktop

A cargo workspace (ADR-0009): `compositor/` is a floating Smithay compositor (ADR-0006, 0007),
`bar/` is a layer-shell egui status bar skinned with Patina, `launcher/` is the app launcher the
bar's Sector F button opens (ADR-0010), and `shell/` (patina-shell) is the layer-shell host and
software painter they share (ADR-0011). The compositor puts its own folder first on its
clients' PATH, so the bar and launcher are found beside it.

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

- `compositor/src/state.rs`: server side: protocols, placement, focus, input into the seat.
- `compositor/src/drag.rs`: three-finger drag (ADR-0031): libinput swipes hold BTN_LEFT, with a
  grace timer on `State::loop_handle` between strokes.
- `compositor/src/screenshot.rs`: Super+Shift+3/4 and Print (ADR-0034): the desktop or the focused
  window drawn offscreen with the backend's renderer, saved as a PNG on a thread; then a `Flash`
  each backend draws on top (`flash_element`, `overlay`).
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
- `shell/src/host.rs`: layer-shell egui host (sctk), generic over a `Shell`; `Options` sets
  anchor, size, keyboard and per-output surfaces (ADR-0011).
- `shell/src/raster.rs`: CPU painter for egui meshes (ADR-0002).
- `launcher/src/apps.rs`: .desktop entries, search, launch; `folders.rs`: folder apps,
  `~/Apps/*.app/app.toml` (ADR-0035, Patina OS ADR-0004's format 1), read by the patina-apps crate
  that Files shares (ADR-0036; a private git dependency over ssh, `.cargo/config.toml`);
  `icons.rs`: theme lookup and decoding on a thread; `ui.rs`: the surface. A second run closes the open one over a socket in
  XDG_RUNTIME_DIR. The bar passes its skin in `PATINA_SKIN`.
- `compositor/src/toplevels.rs` and `shell/src/toplevels.rs`: wlr-foreign-toplevel-management,
  server and client, for the bar's `windows` tabs (ADR-0012).
- `wifi/`: patina-wifi, the tray's Wi-Fi menu (ADR-0022), on `shell/src/network.rs`
  (NetworkManager over the system bus with zbus; `cargo run -p patina-shell --example wifi` prints
  what it reads). `shell/src/piece.rs` is what the bar's pieces share: one copy per display over a
  socket, and the bar's skin and edge.
- `volume/`: patina-volume, the tray's volume menu, also flashed by the compositor after a
  volume key (`--flash`) or a brightness key (`--brightness`) (ADR-0021, ADR-0033).
  `shell/src/audio.rs` (wpctl), `backlight.rs` (sysfs and logind), `icons.rs` and `bar.rs` (the
  bar's skin and edge for pieces the compositor starts) are shared.
- `lock/`: patina-lock, the lock screen (ADR-0028): `Options::lock` makes the host draw on
  ext-session-lock surfaces and unlock when the shell finishes; `pam.rs` declares the PAM calls.
  The compositor's side is `compositor/src/lock.rs` (ADR-0025).
- `menu/`: patina-menu, the desktop's menu on a right click on the desktop (ADR-0029): the skin
  menu, writing `skin` into the bar's config (`shell/src/bar.rs` `set_skin`); the bar polls the
  file's mtime each second and restyles (`Host::restyle`). Its Wallpaper submenu sets `wallpaper`.
- `wallpaper/`: patina-wallpaper (ADR-0030), a click-through background-layer surface: the image,
  or the default flask (`shell/src/logo.rs`, with Ubuntu Mono) in the skin's colours. `cargo test
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
