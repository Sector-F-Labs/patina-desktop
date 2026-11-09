# What the Patina desktop does

What works today, by area, with the ADR that records the decision. For what doesn't yet, and what
comes next, see the [roadmap](ROADMAP.md). Each piece's README says how it works, and `man
patina-desktop` is the overview.

Last checked against the code: 2026-11-09.

## Windows

- **A floating compositor on Smithay** that runs on the hardware (DRM/KMS, GBM, libinput, libseat),
  or nested as a window on another desktop ([ADR-0006](adr/0006-own-compositor.md),
  [0016](adr/0016-hardware-backend.md), [0015](adr/0015-gl-when-nested.md)).
- **Title bars from Patina**: windows that draw no frame get one, X11 and Wayland alike, through
  xdg-decoration ([0077](adr/0077-x11-windows-without-a-frame-get-one.md),
  [0085](adr/0085-wayland-windows-get-frames-through-xdg-decoration.md)). Shadows through KDE's shadow
  protocol ([0112](adr/0112-windows-ask-for-their-shadow-through-the-kde-shadow-protocol.md)).
- **Tiling, as on Ubuntu**: drag to an edge, or Super with an arrow; drag the line between tiles to
  resize both; gaps. A separate process decides it, so it can change without restarting the desktop
  ([0049](adr/0049-tiling-by-dragging-decided-over-d-bus.md), [0053](adr/0053-tiling-decided-by-patina-tiler.md),
  [0054](adr/0054-drag-the-line-between-tiled-windows.md), [0051](adr/0051-gaps.md)). Windows reopen
  where they were ([0065](adr/0065-windows-reopen-where-they-were.md)).
- **Minimise to the bar's tabs**, bring back from the tab or Super+1–9
  ([0084](adr/0084-minimise-to-the-bars-tabs.md)). Alt+Tab goes by most recent use
  ([0127](adr/0127-alt-tab-goes-by-most-recent-use.md)).
- **Fullscreen** over the bar for games and video, with the desktop's keys kept out of the way
  ([0041](adr/0041-fullscreen.md), [0102](adr/0102-print-takes-the-screen-when-something-is-fullscreen.md)),
  and a signal on the session bus when one has the keyboard
  ([0129](adr/0129-the-compositor-says-when-a-fullscreen-window-has-the-keyboard.md)).
- **Xwayland** for X11 apps, with the compositor as their window manager
  ([0040](adr/0040-xwayland.md)).

## The HUD (the bar)

- **Tabs for every window**, with app icons, pinned apps, and a menu per tab
  ([0027](adr/0027-bar-tabs-are-the-window-list.md), [0050](adr/0050-window-tabs-show-app-icons.md),
  [0080](adr/0080-pinned-apps-and-moving-tabs.md)). It can be split into parts placed anywhere along the
  edges, and hide at its edge (Super+H) ([0063](adr/0063-split-bar.md),
  [0111](adr/0111-hud-elements-and-hiding.md)).
- **A system tray** by StatusNotifierItem ([0064](adr/0064-system-tray.md)); media controls from MPRIS
  ([0101](adr/0101-media-controls-in-the-bar.md)); the keyboard layout in use.
- **Menus for**: volume and the media players ([0021](adr/0021-tray-volume-menu-and-super-key.md),
  [0117](adr/0117-the-volume-menu-lists-the-players.md)), Wi-Fi with captive-portal sign-in
  ([0022](adr/0022-wifi-menu.md), [0098](adr/0098-wifi-menu-signs-in-to-captive-portals.md)), the
  clipboard history ([0114](adr/0114-the-compositor-keeps-the-clipboard-history.md)), the clock with a
  month and the day's events ([0082](adr/0082-the-clock-opens-a-month-with-calendars-events.md)), the
  battery ([0024](adr/0024-battery-indicator.md)), and power: lock, log out, restart, shut down
  ([0048](adr/0048-a-power-menu-on-the-bar.md)).
- **An on-screen flash** for the volume and brightness keys
  ([0033](adr/0033-volume-flash-holds-and-shows-brightness.md)).
- **Skins**: the same skins as the Patina apps, Everforest Dark by default, chosen in Settings, with
  Skin Builder to make more; a right click on the desktop sets light or dark
  ([0100](adr/0100-the-desktops-default-skin-is-everforest-dark.md),
  [0105](adr/0105-the-desktop-menu-chooses-the-style-not-the-skin.md),
  [0118](adr/0118-skin-builder.md)). A wallpaper in the skin's colours, or your own
  ([0030](adr/0030-wallpaper.md)).

## Launching and files

- **A launcher** over the installed apps, with folder apps from `~/Apps`, and Super+Space as a search box
  ([0010](adr/0010-launcher-reads-desktop-entries.md), [0035](adr/0035-the-launcher-lists-folder-apps.md)).
- **`patina-raise`**: focus an app's window or start it, for keys such as Alt+B
  ([0058](adr/0058-patina-raise.md)).
- **Files**, the file manager, which is also the session's file chooser through the portal
  ([0106](adr/0106-files-lives-in-this-repository.md)); **Reader** and **Notebook**
  ([0107](adr/0107-reader-and-notebook-live-in-this-repository.md)); **Dbug**, a live view of the session
  bus ([0115](adr/0115-dbug-watches-the-bus-as-a-monitor.md)).
- **Default apps** of its own, chosen in Settings ([0066](adr/0066-the-session-has-default-apps.md),
  [0079](adr/0079-settings-chooses-default-apps.md)).
- **A process manager** on Ctrl+Shift+Escape, a key no config can take, over a fullscreen game too
  ([0045](adr/0045-the-compositor-keeps-the-desktops-processes.md),
  [0088](adr/0088-a-process-manager-on-ctrl-shift-escape.md)).

## Keys and input

- **Super is Cmd**: Super combinations reach apps as Ctrl, or Ctrl+Shift in a terminal
  ([0067](adr/0067-super-is-cmd.md)).
- **The desktop's keys are config**, by action, applied within a second; any key remaps to any other;
  Caps Lock held is a layer of keys ([0069](adr/0069-the-desktops-keys-are-config.md),
  [0075](adr/0075-the-default-keys-are-a-toml-file.md), [0076](adr/0076-any-key-remaps-to-any-other.md),
  [0086](adr/0086-caps-lock-held-is-a-layer.md)).
- **Keyboard layouts** on the bus, switched with Ctrl+Space and managed in Settings
  ([0068](adr/0068-keyboard-layouts-on-the-bus.md)).
- **Touchpad and touch**: three-finger drag; touches go to apps as `wl_touch`
  ([0031](adr/0031-three-finger-drag.md), [0094](adr/0094-touches-go-to-apps.md)).
- **Games** lock or confine the pointer and read raw mouse motion
  ([0043](adr/0043-games-lock-the-pointer.md)).

## Screens

- **Fractional scaling**, chosen from the panel's density ([0018](adr/0018-fractional-scaling.md)).
- **Several monitors**: extend or mirror from a popup (Ctrl+Super+P) that also opens when one is plugged
  in; placement in Settings; monitors remembered by model and a known one switched on without asking
  ([0121](adr/0121-multi-monitor.md), [0124](adr/0124-displays-are-remembered-by-model.md),
  [0126](adr/0126-a-known-monitor-is-switched-on-without-asking.md)).
- **Night Light** through the display's gamma ramp ([0039](adr/0039-night-light.md)), also from the
  command line (`patina-night-light`).
- **The screen turns off** after a while without input, unless something is playing
  ([0047](adr/0047-the-screen-turns-off-without-input.md)).
- **Explicit sync** for NVIDIA's driver, so Chromium and Electron apps aren't shown half-drawn.

## Screenshots and recording

- **Print** offers the screen, a window or a selection, or recording one; Super+Shift+3/4/5 as on a Mac;
  every screenshot also goes to the clipboard ([0034](adr/0034-screenshots-with-the-mac-keys.md),
  [0091](adr/0091-print-offers-the-screen-a-window-or-a-selection.md),
  [0116](adr/0116-screenshots-go-to-the-clipboard-and-clipboard-settings.md)).
- **Screen sharing** for OBS and browsers, through xdg-desktop-portal-wlr and the compositor's
  ext-image-copy-capture ([0092](adr/0092-screen-capture-for-apps-and-recording.md)).

## Text, clipboard and voice

- **Clipboard history** kept by the compositor, shown in an applet, with a Settings page
  ([0114](adr/0114-the-compositor-keeps-the-clipboard-history.md)).
- **Dictation with Whisper on the machine**: hold Ctrl+Super+Alt and speak; the words are typed where you
  were, through the clipboard. A waveform overlay, the GPU through Vulkan, and models, language and
  accuracy settings with downloads in Settings
  ([0070](adr/0070-dictation-with-whisper-on-the-machine.md)–[0074](adr/0074-whisper-runs-on-the-gpu-through-vulkan.md),
  [0119](adr/0119-dictation-settings-and-models.md)). The model loads when it's wanted and is put away
  after a game or video, or when idle, all adjustable
  ([0129](adr/0129-the-compositor-says-when-a-fullscreen-window-has-the-keyboard.md)).
- **An emoji picker** that types the pick like dictation ([0120](adr/0120-the-emoji-picker.md)).

## Security and the session

- **A lock screen** of its own, through PAM, locking before the machine sleeps
  ([0025](adr/0025-lock-screen.md), [0028](adr/0028-patina-lock.md)); your picture from `~/.face`
  ([0104](adr/0104-your-picture-is-dot-face.md)).
- **A polkit agent**, so GParted, Disks and `pkexec` can ask for the password
  ([0128](adr/0128-a-native-polkit-agent.md)).
- **A session supervisor** that restarts the compositor after a crash or on Ctrl+Alt+Shift+Backspace,
  and takes the login profile's environment ([0046](adr/0046-patina-session-restarts-the-compositor.md),
  [0083](adr/0083-the-session-reads-the-login-profile.md)).
- **Light or dark** set in Settings, through GNOME's color-scheme
  ([0087](adr/0087-settings-sets-light-or-dark.md)).

## Settings

One app with pages for Appearance, Bar, Wallpaper, Account, Defaults, Keyboard, Dictation, Clipboard,
Displays, Night Light, Power and Battery (with the battery's history), and Processes
([0038](adr/0038-patina-settings.md)).

## Installing and testing

- **`make install`** into `/usr/local` for every user, **`make install-full`** with the apps the
  desktop was tested with, or **one `.deb`** ([0078](adr/0078-installed-for-every-user.md),
  [0099](adr/0099-one-deb-of-the-desktop-and-its-apps.md), [0122](adr/0122-one-install-the-repositorys-make-install.md)).
- **Test VMs**: a Linux guest with fake hardware and regression tests, a multi-monitor one on vkms, and a
  clean one that installs the `.deb` ([0017](adr/0017-linux-vm-for-the-hardware-backend.md),
  [0096](adr/0096-the-linux-vm-has-fake-hardware.md), [0097](adr/0097-linux-regression-tests-drive-the-vm-from-python.md),
  [0125](adr/0125-a-multi-monitor-test-vm-on-vkms.md)).

## Where it runs

| | |
|---|---|
| Ubuntu 26.04 | Built, installed and run as a session, on hardware and in the VMs |
| Debian 13 | Builds and installs in a clean container; not yet run as a session |
| FreeBSD | The compositor in a window on dwl, in a VM; battery and Wi-Fi too ([0095](adr/0095-battery-and-wi-fi-on-freebsd.md)) |
| Other Linux | Not tried |
