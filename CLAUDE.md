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

Smithay comes from the `patina-0.7` branch of Sector-F-Labs/smithay, a fork (ADR-0044): 0.7.0 with upstream's
`_NET_ACTIVE_WINDOW` fix backported (Wine minimised fullscreen games without it), and its
`set_suspended` writing `WM_STATE` (so a game minimised on Alt+Tab is restored). Fixes go there
as commits on 0.7.0, then `cargo update -p smithay`; drop the `[patch.crates-io]` once a release
has them.

Patina comes from its `develop` branch on GitHub (ADR-0013): change Patina there, push
`develop`, then `cargo update -p patina-ui` here. Never publish Patina without asking.

## Commands

- `make` lists targets; `make check` (workspace tests + clippy -D warnings) before committing.
- `make deploy <host>` builds here and installs on an ssh host (`stage` into `target/deploy` with
  its home's PREFIX, then rsync), with Teleprinter (`../teleprinter` as it is, else a clone from
  GitHub) through its own `make install DESTDIR=`; it never adds the login session there. The
  compositor gives clients `TERMINAL=teleprinter` where it's installed (`clients.rs`).
- `make run` opens the compositor as a window on the current desktop, on a session bus of its own
  (`dbus-run-session`) so it never takes a running Patina session's names (it pops up on the user's
  GNOME session, so prefer the VM).
- Test ground is a FreeBSD VM from `bsdt.toml`: dwl from packages is only the parent desktop;
  `gui.start` runs `target/release/patina-compositor` in it. `make vm`, then `make vm-restart`
  after changes, `make screenshot` (grim → docs/screenshot.png) and read the PNG back.
- The user often uses the VM live over VNC. Don't `bsdt type`/`bsdt key` into it without asking;
  grim screenshots are fine.
- Logs in the guest: /tmp/bsdt-gui.log (compositor, bar and the apps they start), /tmp/bsdt-session.log (dwl).

## Layout

Each crate has a README saying its role on the desktop and how it fits with the others; keep it
current when that changes. Each installed binary has a man page in `man/` (mdoc), and
`patina-desktop.7` is the overview: keys, files, restarting and recovering. Keep them current
with options, keys, config and behaviour; `make check` lints them with mandoc, and `make install`
puts them in `$(PREFIX)/share/man`.

- `compositor/src/state.rs`: server side: protocols, placement, focus, input into the seat.
- `compositor/src/drag.rs`: three-finger drag (ADR-0031): libinput swipes hold BTN_LEFT, with a
  grace timer on `State::loop_handle` between strokes.
- `compositor/src/processes.rs`: what the compositor starts (ADR-0045): a global list, each child in
  a process group of its own with its output read and kept; the wallpaper and bar restarted when
  they exit. `processes/dbus.rs` serves `com.sectorflabs.Patina.Processes`, nested too; Settings'
  Processes page restarts, stops and reads them through `system/src/processes.rs`, and the bar and
  menu start what they open through its `Spawn`, falling back to starting it themselves.
- `session/`: patina-session (ADR-0046), the login session's `Exec=`: runs the compositor and starts
  it again on exit status 75 (`session::RESTART`, Ctrl+Alt+Shift+Backspace or `RestartDesktop`) or a
  crash. `compositor/src/session.rs` is the compositor's side, with the marker that keeps the screen
  locked across a restart.
- `compositor/src/screenshot.rs`: Super+Shift+3/4 and Print (ADR-0034): the desktop or the focused
  window drawn offscreen with the backend's renderer, saved as a PNG on a thread; then a `Flash`
  each backend draws on top (`flash_element`, `overlay`).
- `compositor/src/cmd.rs`: Super is Cmd (ADR-0067): a Super key the compositor doesn't keep is sent
  to the focused client as Ctrl+key (Ctrl+Shift to a terminal, by its desktop entry's category, and
  Super+Shift+key there as Ctrl+Alt+Shift+key),
  press and release, with that client alone told Ctrl is down; called from `State::key`'s filter.
- `compositor/src/night_light.rs`: Night Light (ADR-0039): the colour maths, the `Schedule` trait,
  fades, overrides and inhibitors; `UdevData::set_temperature` sets the CRTC's gamma ramp (hardware
  only). `night_light/dbus.rs` serves `com.sectorflabs.Patina.NightLight`; `dbus-run-session --
  cargo test -p patina-compositor night_light -- --ignored` tests it on a bus of its own. The
  compositor reloads its config within a second of a save.
- `compositor/src/idle.rs`: the screen turning off (ADR-0047): `[screen] off_after` without input
  (`input::handle` and the nested backend call `State::input_activity`), `UdevData::power_off`/`on`
  (DPMS through `DrmCompositor::clear`; nested it only logs), held on by idle-inhibit and by
  `org.freedesktop.ScreenSaver` (`idle/dbus.rs`; `dbus-run-session -- cargo test -p
  patina-compositor idle -- --ignored`).
- `compositor/src/keyboard.rs`: the keymap from `[keyboard]` or `/etc/default/keyboard`; with
  `keyboard/dbus.rs` (ADR-0068), `com.sectorflabs.Patina.Keyboard`: the layouts and the one in use
  (`State::layout_maybe_changed` after each key), and SetCurrent. Client and xkb's catalogue:
  `system/src/keyboard.rs`; the bar's tray shows it (`tray::layout`, `menu::layout_args`) and
  Settings' Keyboard page edits `[keyboard]`, four layouts at most (`MOST_LAYOUTS`: xkb drops the
  rest silently). Super+Space opens the launcher, Ctrl+Space switches.
  `dbus-run-session -- cargo test -p patina-compositor keyboard -- --ignored` tests the service.
- `compositor/src/xwayland.rs`: X11 apps (ADR-0040): Xwayland spawned at start-up, `DISPLAY` for
  clients, and the `XwmHandler` that maps, places, moves and resizes X windows as `Window`s in
  the space. `focus.rs`: the keyboard focus is a `FocusTarget` (Wayland surface or X11 window, so
  the X input focus follows). `window_for` finds both kinds by their `wl_surface`.
- `compositor/src/constraints.rs`: relative-pointer (ADR-0043) and pointer-constraints, so games (and
  Xwayland's pointer-warp emulation) get mouse-look: `State::pointer_moved` sends relative motion
  and holds a locked or confined pointer. Only libinput gives relative motion.
- `compositor/src/tiling.rs`: tiling's mechanism only (ADR-0049, ADR-0053): `MoveGrab` calls
  `drag_moved`/`drag_ended`, Super+arrows `snap_requested`, all signalled on the bus; the preview
  (`preview_element`, drawn with the flash by every backend). `tiling/dbus.rs` serves
  `com.sectorflabs.Patina.Tiling`. The policy is `tiler/` (patina-tiler, kept running like the
  bar): `policy.rs` (zones, halves, snaps, tested) and `main.rs` (the bus). It holds the Tiler
  name allowing replacement (`claim_default`); another tiler `claim`s it. Client:
  `system/src/tiling.rs`, example tiler `system/examples/thirds.rs`. `dbus-run-session -- cargo
  test -p patina-compositor tiling -- --ignored` tests the service on a bus of its own.
  `compositor/src/split.rs` (ADR-0054): the line between tiled `Rect` windows a gap apart
  (`line_under`) shows the arrow (`hover_split` in `pointer_motion`), and `SplitGrab` retiles
  both sides live, then signals `Resized`; the tiler keeps its `split`. Windows reopen where
  their app's was (ADR-0065): the compositor signals `WindowOpened` on the first buffer
  (`window_shown`) and `WindowClosed` with where it floated, and serves `Move`; the tiler's
  `positions.rs` keeps them by app id and `policy::opening` moves it, or fills if too big.
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
- `raise/`: patina-raise (ADR-0058, ADR-0060), focus an app's window or start it, for key bindings
  (Alt+B is `patina-raise firefox` by default; `-i`/`-p` for case and partial matches, also on
  `.desktop` names, which `patina_system::apps` reads): `choose.rs` decides, `main.rs` lists and activates
  windows over wlr-foreign-toplevel with plain wayland-client; `--list` prints app ids.
- `wifi/`: patina-wifi, the tray's Wi-Fi menu (ADR-0022), on `system/src/network.rs`.
- `volume/`: patina-volume, the tray's volume menu, also flashed by the compositor after a
  volume key (`--flash`) or a brightness key (`--brightness`) (ADR-0021, ADR-0033).
  On `system/src/audio.rs` and `backlight.rs`.
- `power/`: patina-power, the bar's power menu (ADR-0048): Lock and Log Out through the
  compositor's `com.sectorflabs.Patina.Session` (`compositor/src/session/dbus.rs`), Restart and
  Shut Down through logind or ConsoleKit2, all in `system/src/session.rs`.
- `lock/`: patina-lock, the lock screen (ADR-0028): `Options::lock` makes the host draw on
  ext-session-lock surfaces and unlock when the shell finishes; `pam.rs` declares the PAM calls.
  The compositor's side is `compositor/src/lock.rs` (ADR-0025).
- `menu/`: patina-menu, the desktop's menu on a right click on the desktop (ADR-0029): the skin
  menu, writing `skin` into the bar's config (patina-shell's `bar.rs` `set_skin`); the bar polls the
  file's mtime each second and restyles (`Host::restyle`). Wallpaper… opens Settings at its page.
- Gaps (ADR-0051): `[gaps]` in the compositor's config is the one place: `State::tiling_area`
  insets the work area and `rect_for` splits halves with the gap; the bar reads the same table
  (`bar/src/gaps.rs`) and floats by layer-shell margin (`Host::set_margin`), which the compositor
  adds to its exclusive zone.
- `settings/`: patina-settings (ADR-0038), an eframe Patina app: Appearance (skin, gaps, icon theme
  through gsettings' `icon-theme` for now, `icons.rs`), Bar (`window_tabs` and
  `position` in the bar's config, which the bar follows live, ADR-0050), Wallpaper (sets
  `wallpaper`) and Night Light (writes `[night_light]` into the compositor's config with
  `toml_edit`; state over D-Bus through `system/src/night_light.rs`), Power (the screen's
  `[screen] off_after`, and power-profiles-daemon through `system/src/power_profiles.rs`) and
  Processes (ADR-0045). Also
  `patina-night-light`.
  `cargo test -p patina-settings preview -- --ignored` with PATINA_SETTINGS_PREVIEW=dir draws
  each page in software, without opening a window.
- `wallpaper/`: patina-wallpaper (ADR-0030), a click-through background-layer surface: the image,
  or the default flask (`system/src/logo.rs`, with Ubuntu Mono) in the skin's colours. `cargo test
  -p patina-wallpaper -- --ignored` with PATINA_WALLPAPER_PREVIEW=dir renders the default per skin.
- `bar/src/dwl.rs`: dwl's status lines; `bar/src/bar.rs`: modules; `bar/src/config.rs`;
  `system/src/apps.rs`: a window's `.desktop` entry by app id, for its tab's icon (`icons.rs`) and
  New Window in the tab's menu (ADR-0055). The tab's menu opens in patina-menu
  (`bar/src/menu.rs`, ADR-0056: `TabStrip::own_menu` reports the right click, and patina-menu
  `--item`s print the choice), since the bar's surface is too thin for egui menus. Tooltips
  likewise (`bar/src/tooltip.rs`, ADR-0057): sites call `tooltip::hover`, and after the delay
  the bar runs `patina-menu --tooltip`, ended by closing its stdin; egui's tooltips are off.
  Split mode (`bar/src/split.rs`, ADR-0063, experimental): `Section`s run as `patina-bar
  --section`, `Spot`s in `config.rs`, `Placement::popup` for where things open on screen, passed
  to pieces as `PATINA_OPEN_AT` (patina-shell's `piece::opening`); patina-shell's
  `Shell::wanted_size` and `output_size` serve it.
  The system tray (`system/src/sni.rs`, ADR-0064): the bar's tray process serves
  `org.kde.StatusNotifierWatcher` and draws apps' StatusNotifierItems (`Bar::tray_items`); menus
  are dbusmenu, opened in patina-menu (`menu::tray_args`). `cargo run -p patina-system --example
  tray` prints what's registered; `dbus-run-session -- cargo test -p patina-system sni --
  --ignored` tests it with a fake app.

## Principle

Don't repeat the UI (ADR-0027): a feature grows where it already lives. The bar's tabs are the
window list (Alt+Tab steps along them, closing gives the keyboard back to the window it came from,
else the neighbouring tab, ADR-0059); there's no separate
switcher.

## Gotchas

- `bsdt exec -- cmd ~/x` expands `~` on the host; use `bsdt exec -- sh -c '... $HOME/x'`.
- dwl blocks if a `-s` command stops reading stdin, so the bar reads stdin on its own thread.
- Paint colours only from `ui.visuals()`, so skins reach them.
- In egui, item_spacing changed inside a row leaks to the next widget; change it in `ui.scope`.
- sctk's `delegate_dispatch2!` blanket impl means extra client objects need a local user-data
  type implementing `Dispatch2`, not `()`.
