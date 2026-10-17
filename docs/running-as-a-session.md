# Running the Patina desktop as your session

How to log in to the Patina desktop instead of GNOME on an Ubuntu machine (or any systemd distro
with GDM), and how to get back if something goes wrong.

## Status: what works today

| Piece | State |
|---|---|
| Running nested, as a window on GNOME or another desktop (`make run`) | Works |
| Running on the hardware itself: one monitor, keyboard, mouse and touchpad, GPU | **Works** (tested on real hardware) |
| A login-screen entry: `make install-session` | Works |
| The session setup (`patina-session --session`) | Partly: the desktop's environment, not yet portals and helpers |
| Restarting the desktop without logging out (`patina-session`) | Works: Ctrl+Alt+Shift+Backspace, or after a crash |

patina-compositor picks its backend when it starts. With a desktop to open a window on
(`WAYLAND_DISPLAY` set) it runs nested in it, with OpenGL ES where there's a GPU
([ADR-0015](adr/0015-gl-when-nested.md)) and in software otherwise
([ADR-0008](adr/0008-nested-software-backend.md)). Without one, at a text console or as the login
session, it drives the machine itself ([ADR-0016](adr/0016-hardware-backend.md)). The hardware
backend works on real machines; try it from a text console first ([below](#trying-it-without-logging-out-of-gnome))
before choosing it at the login screen.

## What the hardware backend does

- **The session:** libseat, through systemd-logind, opens the GPU and input devices without root,
  and hands them back and forth when you switch consoles.
- **The screen:** one monitor, the laptop's own panel if it has one, otherwise the first connected
  monitor, at its preferred mode, through DRM/KMS and GBM, drawn with OpenGL ES. Apps hand over GPU
  buffers through linux-dmabuf. It works on Mesa (Intel, AMD) and NVIDIA's driver alike.
- **Scale:** 2 for dense panels that are still at least 800 points tall at 2x, 1.5 from 140 dpi
  (most laptops), 1 otherwise. Set `PATINA_SCALE=1.25` (or any value from 0.5 to 4) to choose. Apps
  that support fractional scaling (most, except GTK3 ones) draw at the exact scale, so text stays
  sharp at 1.5 ([ADR-0018](adr/0018-fractional-scaling.md)).
- **Input:** keyboards, mice and touchpads through libinput; touchpads tap to click and scroll
  naturally (content follows your fingers) unless the config says otherwise.
- **The pointer:** your cursor theme's arrow (`XCURSOR_THEME`, `XCURSOR_SIZE`), or the app's own
  cursor where it sets one.
- **Keys of its own:** **Ctrl+Alt+F1** to **F12** switch consoles. **Ctrl+Alt+Backspace** quits
  the compositor, the way out whatever state it's in, and **Ctrl+Alt+Shift+Backspace** restarts it
  without logging out, closing every window (as the login session, under patina-session). **Super** on its own opens the launcher,
  **Super+1** to **9** bring forward the window in that tab of the bar,
  **Alt+Tab** and **Alt+Shift+Tab** step to the next and previous tab (the bar is the switcher), **Super+Up/Down** (or
  **K/J**) maximise and restore the focused window, **Super+Left/Right** (or **H/L**) fill the left
  or right half, **Super+Shift** with an arrow grows the focused window that way by a
  tenth of the screen, **Super+Shift+K/J** grow and shrink it, and **Super+Shift+L/H** widen and
  narrow it, around its centre, **Ctrl+Super+Q** (as Ctrl+Cmd+Q on a Mac) or **Super+Escape** lock
  the screen, **Super+W** closes the focused window, **Super+Q** quits its app (closes all its windows), **Super+Space** switches
  keyboard layout, **Super+Shift+R** reloads the config file, and, as on a Mac,
  **Super+Shift+3** (or **Print**) saves a screenshot of the desktop and **Super+Shift+4** one of
  the focused window, in ~/Pictures/Screenshots (ADR-0034). The
  brightness and volume keys work, and the volume menu fades in to show the level (or the
  brightness), staying up while they're pressed. A right click
  on the desktop opens the desktop's menu, where the shell's skin and wallpaper are chosen. On the
  touchpad, three fingers drag, as with macOS's accessibility setting: lift and put them back
  within half a second to carry on, or carry on with one finger to drop (`three_finger_drag` in
  `[touchpad]` turns it off). The pointer is Patina's own, shaped for what's under it.

## Settings

`~/.config/patina-compositor/config.toml`; every key is optional
([ADR-0019](adr/0019-compositor-config-file.md)). The first start writes it with every setting
commented out at its default (from `compositor/config.example.toml`), so it changes nothing until
you edit it, and it's never rewritten once it exists:

```toml
scale = 1.25                 # unset: from the panel's density

[keyboard]
layout = "us,se"             # unset: /etc/default/keyboard
variant = ""
options = ""
repeat_delay = 200           # ms before a held key repeats
repeat_rate = 25             # repeats per second

[touchpad]
tap = true                   # tap to click
natural_scroll = true        # content follows your fingers
speed = 0.0                  # -1.0 (slow) to 1.0 (fast)

[media]
brightness_step = 5          # percent per press of a brightness key
volume_up = "wpctl set-volume -l 1.0 @DEFAULT_AUDIO_SINK@ 5%+"
volume_down = "wpctl set-volume @DEFAULT_AUDIO_SINK@ 5%-"
mute = "wpctl set-mute @DEFAULT_AUDIO_SINK@ toggle"
mic_mute = "wpctl set-mute @DEFAULT_AUDIO_SOURCE@ toggle"
```

`[bindings]` maps key combinations to commands; by default **Alt+T** opens Teleprinter and
**Alt+B** Firefox. A `[bindings]` section of your own replaces those defaults:

```toml
[bindings]
"Alt+T" = "teleprinter"
"Alt+B" = "firefox"
"Super+Return" = "foot"
```

A bound key no longer reaches apps (Alt+B is "back a word" in shells, for example).

The brightness keys need no extra tools: the compositor sets the backlight through the login
session ([ADR-0020](adr/0020-brightness-and-volume-keys.md)). The volume keys run the commands
above, for PipeWire (Ubuntu's sound system).

Press **Super+Shift+R** after saving to apply it without logging out. Mistakes are reported in
the session's log (`journalctl -b | grep patina`) and never stop the desktop: a broken file keeps
the settings in use. `PATINA_SCALE` and `XKB_DEFAULT_LAYOUT` in the environment still win over
the file.

Not yet: more than one monitor (others stay dark), plugging monitors in and out, the lid, an
on-screen display for brightness and volume, and explicit sync for NVIDIA apps.

## You don't need to remove GDM

GDM, like any display manager, offers every session installed on the machine. Installing Patina's
session adds it to the list beside Ubuntu (GNOME), and GNOME stays there as your way back. Removing
GDM gains nothing and loses the fallback.

## Installing the session

```sh
make install-session         # run it without sudo; it asks for your password itself
```

It installs the binaries (patina-compositor, patina-bar and the other pieces, and patina-launcher
from its own repository) into
`~/.local/bin`, and their man pages into `~/.local/share/man` (`man patina-desktop` for the
overview), as `make install` does, then the session entry into
`/usr/share/wayland-sessions/patina.desktop`, which is the only part that needs root: GDM only
reads sessions from that system folder. Don't run it as `sudo make install-session`; it refuses,
since under sudo the binaries would go to root's home.

`make install-session PREFIX=/usr/local` puts the binaries in `/usr/local/bin` instead (then the
binaries need root too, so run `sudo make install PREFIX=/usr/local` first).

The entry it installs, from `packaging/linux/patina.desktop`, names patina-session by its full
path, since a login session doesn't read your shell's `PATH`. patina-session runs the compositor
beside it, passing `--session` on, and starts it again when it restarts or crashes
([ADR-0046](adr/0046-patina-session-restarts-the-compositor.md)):

```ini
[Desktop Entry]
Name=Patina
Comment=The Patina desktop
Exec=/home/you/.local/bin/patina-session --session
Type=Application
DesktopNames=Patina
```

`DesktopNames` becomes `XDG_CURRENT_DESKTOP=Patina` in the session, which is what `.desktop` files'
`OnlyShowIn=` and `NotShowIn=` are checked against (the launcher already treats this desktop as
`Patina`).

### What the session sets up

`--session` is where the compositor does what a session needs before its first client, so there's
no separate shell script to keep in step. GDM, through systemd-logind, already gives the session a
D-Bus session bus and `XDG_RUNTIME_DIR`. On top of that, the compositor:

- sets `XDG_CURRENT_DESKTOP=Patina`, `XDG_SESSION_DESKTOP=patina` and `XDG_SESSION_TYPE=wayland`
  for its clients (always, nested or not);
- on the hardware (from GDM or a text console) or with `--session`, exports those and
  `WAYLAND_DISPLAY` to systemd and D-Bus (`dbus-update-activation-environment --systemd`), so
  services started later find the compositor. Without it, xdg-desktop-portal doesn't know it's in
  Patina, falls back to GTK's backend, which can't find the display, and file choosers never
  open.

Still to do, so these don't work in the session yet:

- start **xdg-desktop-portal** with a backend, so Firefox's file picker, Flatpak apps and screen
  sharing work. `xdg-desktop-portal-wlr` (screen sharing) and `xdg-desktop-portal-gtk` (file
  pickers) work until there's a Patina one;
- a **polkit agent**, for password prompts from Software, Disks and the like;
- a **notification daemon**, until Patina has its own notifications;
- **gnome-keyring**, if you want passwords stored as on GNOME (Ubuntu's PAM setup unlocks it
  at login);
- an autostart list for apps to open at login.

### Restarting the desktop

To try a new build of the compositor, or when the desktop has gone wrong, restart it rather than
logging out: **Ctrl+Alt+Shift+Backspace**, or **Restart Desktop…** at the bottom of Settings'
Processes page. Every window closes and its app quits; the compositor, wallpaper and bar start
again, from the binaries now installed. After `make install`, that's all a change takes.

If the compositor crashes, patina-session starts it again a second later. If it crashes three
times within a minute, the session ends and you're back at the login screen, with patina-session's
reason in the journal. If the screen was locked, the restarted desktop is locked too.

## Choosing Patina at login

1. At the GDM login screen, **click your name** first. The session button only appears on the
   password screen, as a small icon in the **bottom-right corner**, and only when more than one
   session is installed.
2. Choose **Patina**, then enter your password. GDM remembers the choice for next time.

If you never see a password screen, **automatic login** is on: turn it off in Settings → System →
Users, or log out of GNOME to reach the login screen.

### Without the session button

The button only writes a per-user setting, which you can set yourself. AccountsService keeps it in
`/var/lib/AccountsService/users/<your user name>`:

```ini
[User]
Session=patina
```

The value is the session file's name without `.desktop`. Edit it with `sudo`, then log out. Set it
back to `ubuntu` to return to GNOME.

## Trying it without logging out of GNOME

Before trusting it as your session, run it from a text console:

1. Press **Ctrl+Alt+F3** and log in at the text prompt.
2. Run `patina-compositor` (after `make install`; or `target/release/patina-compositor` in the
   repo after `make build`). With no `WAYLAND_DISPLAY` set, it takes the GPU and the screen, and
   starts the bar. Its messages, including the monitor, mode and scale it chose, are on the
   console when it exits.
3. To leave, press **Ctrl+Alt+Backspace** to quit it, or **Ctrl+Alt+F2** (or F1) to switch back
   to GNOME and leave it running; **Ctrl+Alt+F3** returns to it.

A crash here only ends that console's session; GNOME on the other one is untouched. If the screen
stays black, switch consoles: the kernel takes the screen back.

Run inside GNOME's own session (a terminal there, with `WAYLAND_DISPLAY` unset), it can't open the
GPU GNOME holds, so it exits with "Resource temporarily unavailable" and leaves the desktop alone.

## If something goes wrong

- **The session fails to start and drops you back at the login screen.** Choose Ubuntu again. The
  session's output is in the journal: `journalctl -b | grep patina`. Check there for the actual error.
- **The screen is frozen or black.** Switch to a text console (Ctrl+Alt+F3), log in, and run
  `pkill patina-session`; GDM comes back. (`pkill patina-compositor` only restarts the desktop.) If keys don't respond at all, a reboot returns you to
  the login screen, where you can choose Ubuntu.
- **To stop logging in to Patina by default,** choose Ubuntu from the session button, or set
  `Session=ubuntu` in the AccountsService file above.

## Removing it

```sh
make uninstall-session        # the login-screen entry (asks for sudo)
make uninstall                # the binaries; or: sudo make uninstall PREFIX=/usr/local
```

and set `Session=ubuntu` in `/var/lib/AccountsService/users/<your user name>` if it says `patina`.

## Without GDM

If you later want a lighter login screen, `greetd` with `tuigreet` reads the same
`/usr/share/wayland-sessions/` entries. There's no need to change while GDM works, and GNOME
remains a session you can choose.
