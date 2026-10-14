# Running the Patina desktop as your session

How to log in to the Patina desktop instead of GNOME on an Ubuntu machine (or any systemd distro
with GDM), and how to get back if something goes wrong.

## Status: what works today

| Piece | State |
|---|---|
| Running nested, as a window on GNOME or another desktop (`make run`) | Works |
| Running on the hardware itself: screens, keyboard and mouse, GPU | **Not built yet** |
| A login-screen entry, and the session setup it starts | Not built yet; described below |

patina-compositor only has a nested backend so far ([ADR-0008](adr/0008-nested-software-backend.md)):
it opens a window on another Wayland desktop and draws into it in software. To be your session it
needs a second backend that drives the machine directly, and until that exists, the steps from
[Installing the session](#installing-the-session) on won't give you a working desktop. They're
written down now so the shape is settled; the commands will work as soon as the backend lands.

## What the hardware backend needs

Smithay, which the compositor is built on, provides all of it; its `anvil` example and niri show
the full wiring.

- **libseat**, for access to the GPU and input devices through the login session. GDM starts
  sessions through systemd-logind, which hands those over without root.
- **DRM/KMS**, to find the monitors, set their modes and show frames on them.
- **libinput**, for keyboards, mice and touchpads, with the keyboard layout from your settings.
- **A GPU renderer** (Smithay's GLES renderer) in place of pixman, and **linux-dmabuf**, so apps
  hand the compositor GPU buffers. This also ends software rendering for apps.

The plan is a `compositor/src/udev.rs` beside `nested.rs`, chosen at start-up: nested when
`WAYLAND_DISPLAY` is set (there's a desktop to open a window on), hardware otherwise.

## You don't need to remove GDM

GDM, like any display manager, offers every session installed on the machine. Installing Patina's
session adds it to the list beside Ubuntu (GNOME), and GNOME stays there as your way back. Removing
GDM gains nothing and loses the fallback.

## Installing the session

### 1. The binaries

```sh
make install                 # patina-compositor, patina-bar and patina-launcher into ~/.local/bin
```

A login session doesn't read your shell's `PATH` changes, so either install system-wide
(`sudo make install PREFIX=/usr/local`) or make sure `~/.local/bin` is on `PATH` in
`~/.profile`, which Ubuntu's does by default.

### 2. The session entry

GDM lists the `.desktop` files in `/usr/share/wayland-sessions/`. That folder is system-wide (a
session in `~/.local/share` isn't read), so it needs root:

```ini
# /usr/share/wayland-sessions/patina.desktop
[Desktop Entry]
Name=Patina
Comment=The Patina desktop
Exec=patina-compositor --session
Type=Application
DesktopNames=Patina
```

`DesktopNames` becomes `XDG_CURRENT_DESKTOP=Patina` in the session, which is what `.desktop` files'
`OnlyShowIn=` and `NotShowIn=` are checked against (the launcher already treats this desktop as
`Patina`).

`--session` doesn't exist yet. It's where the compositor would do what a session needs before it
starts its first client (below), so there's no separate shell script to keep in step.

### 3. What the session sets up

GDM, through systemd-logind, already gives the session a D-Bus session bus and
`XDG_RUNTIME_DIR`. On top of that, `patina-compositor --session` should:

- set `XDG_CURRENT_DESKTOP=Patina`, `XDG_SESSION_DESKTOP=patina` and `XDG_SESSION_TYPE=wayland`
  for its clients, and export `WAYLAND_DISPLAY` to systemd and D-Bus
  (`dbus-update-activation-environment --systemd WAYLAND_DISPLAY XDG_CURRENT_DESKTOP`), so
  services started later find the compositor;
- start **xdg-desktop-portal** with a backend, so Firefox's file picker, Flatpak apps and screen
  sharing work. `xdg-desktop-portal-wlr` (screen sharing) and `xdg-desktop-portal-gtk` (file
  pickers) work until there's a Patina one;
- start a **polkit agent**, for password prompts from Software, Disks and the like;
- start a **notification daemon**, until Patina has its own notifications;
- start **gnome-keyring** if you want passwords stored as on GNOME (Ubuntu's PAM setup unlocks it
  at login);
- then start patina-bar, and anything in the autostart list.

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
2. Run `patina-compositor`. With no `WAYLAND_DISPLAY` set, it would take the hardware backend.
3. To leave, quit the compositor, or switch back to GNOME with **Ctrl+Alt+F2** (GDM's sessions
   usually live on F1 or F2; try the others if needed).

A crash here only ends that console's session; GNOME on the other one is untouched.

Until the hardware backend exists, `make run` inside GNOME is the way to use the desktop: it runs
nested, as a window.

## If something goes wrong

- **The session fails to start and drops you back at the login screen.** Choose Ubuntu again. The
  session's output is in the journal: `journalctl --user -b` (or `journalctl -b` and search for
  `patina`).
- **The screen is frozen or black.** Switch to a text console (Ctrl+Alt+F3), log in, and run
  `pkill patina-compositor`; GDM comes back. If keys don't respond at all, a reboot returns you to
  the login screen, where you can choose Ubuntu.
- **To stop logging in to Patina by default,** choose Ubuntu from the session button, or set
  `Session=ubuntu` in the AccountsService file above.

## Removing it

```sh
sudo rm /usr/share/wayland-sessions/patina.desktop
make uninstall                # or: sudo make uninstall PREFIX=/usr/local
```

and set `Session=ubuntu` in `/var/lib/AccountsService/users/<your user name>` if it says `patina`.

## Without GDM

If you later want a lighter login screen, `greetd` with `tuigreet` reads the same
`/usr/share/wayland-sessions/` entries. There's no need to change while GDM works, and GNOME
remains a session you can choose.
