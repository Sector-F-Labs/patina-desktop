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
`_NET_ACTIVE_WINDOW` fix backported (Wine minimised fullscreen games without it), its
`set_suspended` writing `WM_STATE` (so a game minimised on Alt+Tab is restored), and upstream's
ext-image-capture-source and ext-image-copy-capture modules (ADR-0092), without their
`duplicate_frame` error, which xdg-desktop-portal-wlr trips. Fixes go there as commits on 0.7.0 (the clone beside this repo, `../smithay`),
then `cargo update -p smithay`; drop the `[patch.crates-io]` once a release has them.

Patina comes from its `develop` branch on GitHub (ADR-0013): change Patina there, push
`develop`, then `cargo update -p patina-ui` here. Never publish Patina without asking.

## Commands

- `make` lists targets; `make check` (workspace tests + clippy -D warnings) before committing.
- `make install` (ADR-0078) builds as you, then runs itself again through doas (else sudo) to
  install for every user: `/usr/local`, the session entry in `/usr/share/wayland-sessions`
  (FreeBSD: under PREFIX) and `/etc/pam.d/patina-lock`. As root it never builds. `make deploy
  <host>` builds here and installs the same on an ssh host (`stage` into `target/deploy`, then
  `rsync --rsync-path="doas rsync"`), with Teleprinter (`../teleprinter` as it is, else a clone
  from GitHub) through its own `make install DESTDIR= PREFIX=/usr/local`. The
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
- The Linux test VM (ADR-0017, `vm-linux/`): Ubuntu of the host's release in QEMU, booting into a
  Patina session on tty1 (the hardware backend, on a virtio GPU drawn in software), on this
  checkout's `target/release` shared in read-only. `make vm-linux`, `make vm-linux-restart` after
  changes, `make vm-linux-shot` (→ target/vm-linux/screen.png) and read the PNG back;
  `vm-linux/vm.sh ssh` for a shell. Session log in the guest: /tmp/patina-session.log.

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
- `session/`: patina-session (ADR-0046), the login session's `Exec=`: takes the environment of
  `/bin/sh -l` (`~/.profile`'s PATH, ADR-0083; five seconds, else goes on without), runs the compositor and starts
  it again on exit status 75 (`session::RESTART`, Ctrl+Alt+Shift+Backspace or `RestartDesktop`) or a
  crash. `compositor/src/session.rs` is the compositor's side, with the marker that keeps the screen
  locked across a restart.
- `compositor/src/screenshot.rs`: Super+Shift+3/4 (ADR-0034): the desktop or the focused
  window drawn offscreen with the backend's renderer, saved as a PNG on a thread; then a `Flash`
  each backend draws on top (`flash_element`, `overlay`).
  Print and Super+Shift+5 (ADR-0091) start `screenshot/` (patina-screenshot), a patina-shell
  pill offering screen, window or selection, which prints the choice and exits;
  `screenshot/menu.rs` reads it, and `screenshot/select.rs` is the drag-out selection, drawn
  as solid-colour elements beside the flash, with the pointer kept from the windows.
  `PATINA_SCREENSHOT_PREVIEW=dir cargo test -p patina-screenshot preview -- --ignored` draws it.
- `compositor/src/capture.rs`: screen capture for other programs (ADR-0092): ext-image-copy-capture
  of the screen or one window (`Target`), ext-foreign-toplevel-list (`toplevels::refresh`) to
  pick one. A frame waits on its session's damage tracker, then the `Scene` is drawn offscreen
  (`screenshot::read_back`) and copied into the client's shm buffer; `State::capture_frames`
  runs after each pass of the loop. xdg-desktop-portal-wlr is the client that matters (OBS,
  browsers): `packaging/portal/` configures it, with `patina-screenshot --share` as its chooser.
  `capture/record.rs` is the desktop's own recording (Record Screen/Selection in the Print
  pill): frames piped to `ffmpeg`, `patina-screenshot --recording` showing it. `cargo run -p
  patina-compositor --example capture -- out.png [--window APP] [--frames N]` checks capture.
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
- `compositor/src/keys.rs`: the desktop's own keys (ADR-0069, ADR-0075): `compositor/keys.toml`,
  built in, gives each action its default keys (`action` maps names to `KeyAction`s), and also
  fills the starter config's `[keys]`; `[keys]` in the config replaces an action's, `Keys::new` builds them on every config load
  (live), `Keys::lookup` matches exact modifiers on the base keysym, `Keys::tap` is Super alone.
  Only Alt+Tab, quit, restart, consoles and media keys stay fixed in `input::key_action`.
  `[remap]` (`Keys::remap`, ADR-0076) sends a key to the focused window as another;
  `compositor/src/remap.rs` puts the sent key through the seat and fakes its modifiers.
  `compositor/src/caps.rs` (ADR-0086): with `caps_hold = "layer"`, Caps Lock never reaches the
  seat; keys pressed with it are `[caps_layer]` remaps, and a tap sends `caps_tap` on release.
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
- `compositor/src/frames.rs`: frames for X11 windows that draw none (ADR-0077) and Wayland windows
  in xdg-decoration's server side (ADR-0085, the `XdgDecorationHandler`, which gives the mode
  asked for, server side by default): `wants_frame`
  (not override-redirect, no `_MOTIF_WM_HINTS` without decorations, a normal window or dialog),
  `TITLE` above the window's own place in the space, so `frames::inner`/`outer` convert for
  placement (`maximize.rs`'s `configure_placed`). The bar is patina-ui's `TitleBar` run in an
  egui context per window and painted with patina-shell's painter; its viewport commands are
  carried out (`frame_commands`). Every backend draws `desktop_elements`, Smithay's
  `space_render_elements` with the frames and borders beside their windows. Skin from the bar's
  config file (`bar_skin`), polled each second. `PATINA_FRAMES_PREVIEW=dir cargo test -p
  patina-compositor frames -- --ignored` paints one per skin.
- `compositor/src/minimize.rs`: minimising (ADR-0084): a minimised window is out of the space,
  kept in `State::minimized` with its place; `window_for`, the foreign toplevels and `quit_app`
  still see it, and `State::focus` restores it, so its tab, Alt+Tab and Super+N bring it back.
  Super+M, xdg and foreign-toplevel `set_minimized`, X11 requests (Wine's games) and frames'
  button minimise.
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
- `clock/`: patina-clock, the clock's menu (ADR-0082): a month and the day's events, which the
  Calendar app (`../calendar`) prints with `calendar --agenda` (`agenda.rs`); a click runs
  `calendar --open DATE [--event UID]`. The menu draws, Calendar keeps the data.
- `volume/`: patina-volume, the tray's volume menu, also flashed by the compositor after a
  volume key (`--flash`) or a brightness key (`--brightness`) (ADR-0021, ADR-0033).
  On `system/src/audio.rs` and `backlight.rs`.
- `power/`: patina-power, the bar's power menu (ADR-0048): Lock and Log Out through the
  compositor's `com.sectorflabs.Patina.Session` (`compositor/src/session/dbus.rs`), Restart and
  Shut Down through logind or ConsoleKit2, all in `system/src/session.rs`.
- `dictate/`: patina-dictate (ADR-0070, ADR-0072), speech to text with whisper.cpp (`whisper-rs`).
  `--daemon` (`daemon.rs`), kept running by the compositor: records while the `dictate` key
  (Ctrl+Super+Alt, modifiers alone: `Keys::chord`) is held, between the `DictationStarted`/`DictationStopped` signals, and calls
  Commit, with the overlay (`overlay.rs`, ADR-0073): `--overlay bottom|top`, a patina-shell
  pill with a waveform, started at each press and fed levels (RMS, a line each) then
  `transcribing` on stdin, faded out when stdin closes; `PATINA_DICTATE_PREVIEW=dir cargo test
  -p patina-dictate preview -- --ignored` draws it. Its edge is `overlay` in
  `~/.config/patina-dictate/config.toml` (`config.rs`), which Settings' Dictation page writes.
  Without it, a command that records until Enter and prints the text. `capture.rs` is
  cpal on Linux, `/dev/dsp` on FreeBSD; `transcribe.rs` drops segments Whisper calls no speech.
  Models in `$XDG_DATA_HOME/patina-dictate/models`. On the GPU through Vulkan by default (ADR-0074; CPU
  without a driver), `--features cuda` adds CUDA; `-v` says which device.
- `compositor/src/typing.rs`: text typed into the focused window (ADR-0071), served as
  `com.sectorflabs.Patina.Input` `Commit(text)` (`typing/dbus.rs`; `dbus-run-session -- cargo
  test -p patina-compositor typing -- --ignored`). Through the clipboard: the app's clipboard is
  read out first (`save_clipboard`), the text set as the compositor's selection (`Served`, the
  `SelectionUserData`), Ctrl+V or Ctrl+Shift+V pressed through `KeyboardHandle::input` past the
  compositor's keys, and the old clipboard given back after the window reads it. Wayland only.
  The `dictate` key's press and release are signalled there (`dictate_key_pressed`/`_released`,
  called from `State::key`).
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
- `settings/`: patina-settings (ADR-0038), an eframe Patina app: Appearance (skin, light or dark
  through gsettings' `color-scheme`, `color_scheme.rs`, ADR-0087, gaps, icon theme
  through gsettings' `icon-theme` for now, `icons.rs`), Bar (`window_tabs` and
  `position` in the bar's config, which the bar follows live, ADR-0050), Wallpaper (sets
  `wallpaper` and `wallpaper_fit`, `system/src/wallpaper.rs`) and Night Light (writes `[night_light]` into the compositor's config with
  `toml_edit`; state over D-Bus through `system/src/night_light.rs`), Power (the screen's
  `[screen] off_after`, and power-profiles-daemon through `system/src/power_profiles.rs`),
  Defaults (`defaults.rs`, ADR-0079: an app per role in `~/.config/patina/defaults.toml`,
  `system/src/defaults.rs` writing it out as `~/.config/patina-mimeapps.list`, also at the
  compositor's start), Dictation (the overlay's edge in patina-dictate's config, `dictation.rs`) and
  Processes (ADR-0045). Also
  `patina-night-light`.
  `cargo test -p patina-settings preview -- --ignored` with PATINA_SETTINGS_PREVIEW=dir draws
  each page in software, without opening a window.
- `processes/`: patina-processes (ADR-0088), the process manager, on Ctrl+Shift+Escape (fixed in
  `input::key_action`, works over fullscreen; `open_process_manager` raises its window by app id
  `com.sectorflabs.ProcessManager`, else starts it). `sample.rs` reads processes, CPU, memory,
  disks and network with `sysinfo` on a thread and sends signals; `groups.rs` makes apps of
  the processes with windows, from the compositor's `Windows()`/`Activate(pid)` on the Tabs
  service (`State::windows_with_pids` in `toplevels.rs`; client `system/src/windows.rs`);
  `gpu.rs` reads GPUs as nvtop does (NVML; DRM fdinfo and sysfs; `cargo test -p patina-processes
  gpus_here -- --ignored --nocapture` prints this machine's);
  `overview.rs` (where it opens), `processes.rs` and `performance.rs` are the three views, as
  Patina's fixed tabs; the Overview's busiest lists hold still through `ranking.rs`; `graph.rs`
  draws the graphs (CPU and GPU in their maker's colour, the one exception to skin colours).
  A process's own window (ADR-0089) is `window.rs`, an immediate viewport per process
  (`App::process_windows`), with `details.rs` reading `/proc/<pid>` on the reader thread for
  the open ones (`Ask::Watch`).
  The kernel's log and pressure (ADR-0090): `kernel_log.rs` (journal, `/dev/kmsg`, `dmesg`;
  `classify` finds the events; `cargo test -p patina-processes kernel_log_here -- --ignored
  --nocapture` reads this machine's), `pressure.rs`, `log_view.rs` (the Log tab), and
  `problems.rs` for the Overview's Problems.
  `PATINA_PROCESSES_PREVIEW=dir cargo test -p patina-processes preview -- --ignored` draws each
  view and a process window.
- `wallpaper/`: patina-wallpaper (ADR-0030), a click-through background-layer surface: the image,
  or the default flask (`system/src/logo.rs`, with Ubuntu Mono) in the skin's colours. `cargo test
  -p patina-wallpaper -- --ignored` with PATINA_WALLPAPER_PREVIEW=dir renders the default per skin.
- Pinned apps and dragging tabs (ADR-0080): `pinned` in the bar's config; `system/src/pins.rs`
  `arrange`s the tabs (pins first, a launcher for one not open) for the bar and the compositor
  alike (`toplevels.rs` `order`/`nth`; Super+N on a launcher `start_app`s it). A drop is decided
  in `bar/src/pins.rs`: pins rewritten, or a window `Move`d by the compositor
  (`compositor/src/tabs/dbus.rs`, `com.sectorflabs.Patina.Tabs`), whose `Moved` the bar
  applies to its own list. Patina's `TabStrip::movable`/`launchers` draw it.
  `dbus-run-session -- cargo test -p patina-compositor tabs -- --ignored` tests the service.
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
