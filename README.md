# Patina desktop

A floating Wayland desktop for Ubuntu and FreeBSD, in the style of
[Patina](https://github.com/Sector-F-Labs/patina) apps. A compositor, a bar, a launcher, a lock
screen, menus, a process manager and the apps it was tested with (a terminal, files, notes, a
reader, paint, video and music), each its own small program, so one going wrong can be restarted
without the others. Windows float, tile when you drag them to an edge, and have tabs in the bar.

![The Patina desktop: Reader showing the patina-desktop man page, tiled beside the Process Manager, with the bar along the bottom](docs/screenshot.png)

## Try it

Two ways: on a real Ubuntu installation, or in one of the virtual machines this repository can
boot, which touch nothing on your machine. To work on the desktop, see [Development](#development).

### On an Ubuntu installation

The whole desktop is one `.deb`, built from this repository and the apps `packaging/bundle.toml`
pins, each at a commit it was tested at. It is built for Ubuntu 26.04 on the machine it's built on,
and fetches the apps from their GitHub repositories, so it needs the network.

```sh
git clone https://github.com/Sector-F-Labs/patina-desktop && cd patina-desktop
make deps                  # lists the packages to build with; install them, dpkg-dev, and Rust (rustup)
make deb                   # builds target/deb/patina-desktop_<version>_amd64.deb
sudo apt install ./target/deb/patina-desktop_*.deb
```

Log out, and choose **Patina** from the session menu (the gear) on the login screen. Super opens
the launcher, Alt+T the terminal, Ctrl+Shift+Escape the process manager.
Ctrl+Alt+Shift+Backspace restarts the desktop without logging out, and Ctrl+Alt+Backspace quits
it, back to the login screen. `sudo apt remove patina-desktop` takes it all away again.
[docs/running-as-a-session.md](docs/running-as-a-session.md) has how to get back if something
goes wrong.

On another distribution, or into another prefix, `make install-full` installs the same set from
source into `/usr/local`, and `make install` the desktop alone, without the apps.

### In a virtual machine

The **Linux VM** is an Ubuntu guest in QEMU with a fake battery and Wi-Fi, running a Patina
session on its screen (needs `qemu-system-x86_64` with KVM, and `uv` for the tests; the first
start downloads Ubuntu's cloud image, which is kept in `~/.cache/patina-desktop`):

```sh
make vm-linux              # build here, boot the guest on those binaries, start the session
make vm-linux-gui          # its screen in a VNC viewer (localhost:5903)
make vm-linux-restart      # after a change: rebuild and restart its session
vm-linux/vm.sh ssh         # a shell in it; vm-linux/vm.sh fake battery 15 discharging
make test-vm               # the regression tests: the session, the bar's battery and Wi-Fi, the apps
make vm-linux-stop         # power it off
```

The **rc VM** is a second, clean guest with no shared folders that installs the `.deb`, so what
passes is what ships. Run one VM at a time: both use the same ports.

```sh
make deb
make test-vm-rc            # boots it, installs the .deb, runs the same tests
PATINA_VM_INSTANCE=rc vm-linux/vm.sh view   # its screen, while it runs
```

On FreeBSD, `make vm` boots the compositor in a window on dwl, in a [bsdt](https://bsdt.divanv.com)
VM (`make vm-gui` to look at it, `make screenshot` to save its screen).

## Using it

**Nested** (on GNOME, sway and so on): the compositor opens a window; move it with Super+drag.
Windows float and cascade; click to focus and raise.

**On hardware** (a text console, or the login screen after installing): one monitor (the internal
panel first), the GPU through Mesa or NVIDIA, fractional scaling chosen from the panel's density
(`scale` in the config overrides it), keyboard, mouse, touchpad and touchscreen through libinput,
console switching (Ctrl+Alt+F1–F12).

The desktop's keys (change them in `[keys]` in the compositor's config, applied within a second,
ADR-0069; the defaults are in [compositor/keys.toml](compositor/keys.toml)):

| Keys | |
|---|---|
| **Super**, **Super+Space** | the launcher |
| **Alt+T**, **Alt+B** | Teleprinter; Firefox, or its window if it's open |
| **Alt+Tab**, **Alt+Shift+Tab** | step along the bar's tabs (fixed) |
| **Super+1–9** | bring a tab forward |
| **Super+M** | minimise to its tab; focusing the tab brings it back |
| **Super+Q** | quit the app (**Super+W** and other Super keys reach the app as Ctrl, so Super+W closes a tab) |
| **Super+Up/Down** | fill the screen as a tile, restore (an app's own maximise button maximises, ADR-0052) |
| **Super+Left/Right**, **Ctrl+Super+K/J/H/L** | tile to a half or a side |
| **Super+Shift+arrows** | grow or shrink a placed window by 1/10 of the screen |
| **Ctrl+Super+Shift+K/J/L/H** | resize around the centre |
| **Ctrl+Space** | switch keyboard layout |
| **Ctrl+Super+Q**, **Super+Escape** | lock the screen |
| **Ctrl+Super+Alt** (held) | dictate |
| **Super+Shift+3**, **Super+Shift+4** | screenshot the desktop, the focused window (to `~/Pictures/Screenshots`) |
| **Print**, **Super+Shift+5** | choose the screen, a window or a selection, or record one |
| **Ctrl+Shift+Escape** | the process manager (fixed) |
| **Super+Shift+R** | reload the config |
| **Ctrl+Alt+Backspace**, **Ctrl+Alt+Shift+Backspace** | quit, restart the desktop (fixed) |

Dragging a maximised or tiled window restores it under the pointer. The brightness and volume
keys work, with an on-screen flash.

## What's in it

- **[patina-compositor](compositor/README.md)** (`compositor/`): a floating Wayland compositor built on [Smithay](https://github.com/Smithay/smithay). It runs **nested** in a window on another desktop (OpenGL ES, or `PATINA_RENDERER=pixman` in software), or **on the hardware** as a login session (DRM/KMS, GBM, GLES, libinput, libseat). It owns the desktop's keys, minimises, maximises and fullscreens windows, and starts and keeps running the other pieces.
- **[patina-session](session/README.md)** (`session/`): the login session. It runs the compositor and starts it again when it restarts (Ctrl+Alt+Shift+Backspace) or crashes, so neither means logging in again.
- **[patina-bar](bar/README.md)** (`bar/`): a status bar in egui on a wlr-layer-shell surface, skinned with the same skins as the Patina apps: the launcher button, the pinned apps and a tab per window (drag them to reorder), the tray (apps' status icons, keyboard layout, Wi-Fi, volume, battery), the clock and the power button. It runs on any layer-shell compositor, including dwl, where it also shows dwl's tags.
- **[patina-launcher](https://github.com/Sector-F-Labs/patina-launcher)** (its own repository): opened by the bar's Sector F button or the Super key. It lists the installed apps from their `.desktop` files, and folder apps in `~/Apps` (each `Name.app` with an `app.toml`, ADR-0035), and filters them as you type.
- **[patina-tiler](tiler/README.md)** (`tiler/`): how windows tile: dragged to an edge, or with Super and an arrow; it reopens an app's window where it was.
- **[patina-raise](raise/README.md)** (`raise/`): focuses an app's window, or starts the app: Alt+B for Firefox.
- **[patina-menu](menu/README.md)** (`menu/`): the desktop's menu, on a right click on the empty desktop, where the shell's skin and wallpaper are chosen. It is also what the bar's menus and tooltips open in.
- **[patina-wallpaper](wallpaper/README.md)** (`wallpaper/`): the wallpaper, on the background layer of every screen: the bar config's `wallpaper` image, or by default the Sector F Labs flask in the skin's colours.
- **[patina-volume](volume/README.md)** (`volume/`): the volume menu, opened by the bar's speaker; it also flashes up when the volume or brightness keys are used.
- **[patina-wifi](wifi/README.md)** (`wifi/`): the Wi-Fi menu, through NetworkManager: networks in range, joining, disconnecting, Wi-Fi on and off, and signing in to captive portals (ADR-0098).
- **[patina-clock](clock/README.md)** (`clock/`): the clock's menu: a month and the day's events from the Calendar app.
- **[patina-power](power/README.md)** (`power/`): the power menu after the clock: Lock, Log Out, Restart and Shut Down, each but Lock asking first.
- **[patina-lock](lock/README.md)** (`lock/`): the lock screen, in the bar's skin, with the password checked through PAM. The compositor runs it on Ctrl+Super+Q, Super+Escape, when the screen turns off and before sleep.
- **[patina-screenshot](screenshot/README.md)** (`screenshot/`): what the Print key opens: the screen, a window or a selection, or recording the screen or a selection to `~/Videos/Screen Recordings`. Also what an app asking to share the screen may see, through xdg-desktop-portal-wlr.
- **[patina-dictate](dictate/README.md)** (`dictate/`): speech to text on the machine, with Whisper (ADR-0070): hold Ctrl+Super+Alt and speak, and what was said is typed into the focused window.
- **[patina-settings](settings/README.md)** (`settings/`): the skin, wallpaper, default apps, Night Light, power modes, gaps, the bar's layout, and the desktop's processes.
- **[patina-processes](processes/README.md)** (`processes/`): the process manager, on Ctrl+Shift+Escape, a key no config can take: the apps with their windows, and every process, with what each costs the CPU, memory and disks; ending, killing and pausing them; and graphs over the last minute.
- **patina-night-light** (`night-light/`): Night Light from the command line.
- **[patina-system](system/README.md)** (`system/`): what the pieces read and change on the machine (volume, Wi-Fi, battery, backlight), and the tray's icons.
- **[patina-shell](https://github.com/Sector-F-Labs/patina-shell)** (its own repository): what every shell piece is built on: a layer-shell host for egui drawn in software, and the bar's skin and edge.

Each crate's README says what it does on the desktop and how it fits with the others. Each
program has a man page, installed with the desktop; `man patina-desktop` is the overview: the
pieces, the keys, where settings are kept, and restarting and recovering.

The `.deb` and `make install-full` add Teleprinter (the terminal), Files (also the session's file
chooser, through the portal), Reader, Notebook, Draw, Montage and UnAmp, from their own
repositories.

## The bar's config

`~/.config/patina-bar/config.toml`, or `patina-bar -c <file>`. Every key is optional; these are
the defaults:

```toml
position = "bottom"         # or "top"
height = 41                 # as tall as a Patina title bar
skin = "Default"            # a Patina skin, or one in ~/.config/patina-bar/skins
left = ["launcher", "windows", "tags", "layout"]
center = ["title"]
right = ["tray", "clock", "power"]
window_tabs = "both"        # "icons" or "titles"
pinned = []                 # apps pinned to the tabs, by app id
clock = "%a %-d %b  %H:%M"  # strftime, as in date(1)
launcher = "patina-launcher"  # what the Sector F button runs, through sh(1)
volume = "patina-volume"
wifi = "patina-wifi"
calendar = "patina-clock"
battery = "patina-settings --page power"
power = "patina-power"
wallpaper = ""              # an image for patina-wallpaper; "" for the Sector F default
wallpaper_fit = "fill"      # or "fit", "centre"
```

The modules are `launcher`, `windows` (the open windows as Patina tabs: a click brings one
forward, its cross closes it), `tray` (apps' status icons, the keyboard layout when there are
several, Wi-Fi, the speaker and, on laptops, the battery), `clock`, `power` and `title`; under dwl
`tags`, `layout` and `appid` too. `windows` needs a compositor with
wlr-foreign-toplevel-management, such as patina-compositor or sway. Unknown keys and modules are
reported on stderr and skipped, and a broken file falls back to the defaults. Settings' pages
change most of these for you; `layout = "split"` (experimental) places the launcher, tabs and
tray apart ([bar/README.md](bar/README.md)).

## The compositor's config

`~/.config/patina-compositor/config.toml`; every key is optional (ADR-0019). First start writes
it with the defaults commented out, and it is never rewritten; edit it and it applies within a
second, or at once with Super+Shift+R. [compositor/config.example.toml](compositor/config.example.toml)
documents every key:

```toml
scale = 1.25                 # unset: from the panel's density

[keyboard]
layout = "us,se"             # unset: /etc/default/keyboard
caps_lock = "escape"         # Caps Lock tapped; held, a layer of keys (caps_hold)

[touchpad]
tap = true
natural_scroll = true
three_finger_drag = true

[screen]
off_after = 300              # seconds without input; 0 for never

[night_light]
mode = "schedule"            # "off", "always" or "schedule", from `from` to `to`

[gaps]
enabled = true               # space around tiled windows; the bar floats too

[keys]                       # the desktop's own keys, by action
minimise = "Super+M"

[remap]                      # a key pressed, sent to the focused window as another
"Alt+T" = "Ctrl+T"

[bindings]                   # keys that run commands; replaces the defaults
"Alt+T" = "teleprinter"
"Alt+B" = "patina-raise firefox"
```

## Not yet

- **Multiple monitors**, hotplug, lid close/open (it drives the first monitor).
- **Virtual desktops**: there is one workspace (the bar's tags are for dwl).
- **Flatpak integration** through portals. (Screen sharing goes through xdg-desktop-portal-wlr,
  ADR-0092, and files through Files' chooser.)
- **Polkit agent** for auth prompts (Software, Disks, etc.).
- **Notification daemon**.
- **GNOME keyring** integration (PAM unlock at login).
- **Autostart** list for apps at login.

## Development

The quickest way to try a change is the compositor **nested**: a window on a Wayland desktop such
as GNOME or sway, with the bar and wallpaper inside it. Nothing is installed and you stay logged in.

```sh
make run                          # builds, then the compositor in a window, with the bar, on a session bus of its own
patina-compositor foot firefox    # (target/debug/) and any clients to start besides the bar
make run-bar                      # only the bar, on the current compositor (needs layer-shell, so not GNOME)
dwl -s patina-bar                 # the bar alone, on dwl, with dwl's tags
make check                        # tests, clippy, and the man pages linted
```

Windows inside it float and cascade; click to focus and raise, and Super+drag moves one. Some
things are for the hardware only, such as console switching and Night Light, so try those in the
[Linux VM](#in-a-virtual-machine) or on a text console.

## Build

`make` lists the targets:

| Target | |
|---|---|
| `make run` | The compositor in a window on this desktop |
| `make check` | Tests and clippy |
| `make install` | Build, then install **this repository** for every user into `/usr/local`, with the login screen's session and the lock screen's PAM service; asks doas or sudo ([how](docs/running-as-a-session.md), ADR-0078). The apps from other repositories (a terminal, files, notes) aren't in it: that's `install-full` |
| `make install-full` | `make install`, plus the apps `packaging/bundle.toml` pins (Teleprinter, Files, Reader, Notebook, Draw, Montage, UnAmp), each cloned from its GitHub repository and built at its pinned commit, so it needs the network (ADR-0099) |
| `make uninstall-full` | Remove everything `install-full` put in place, from the list of files it left in `/usr/local/share/patina-desktop/files` |
| `make deb` | The whole desktop as a `.deb` in `target/deb` (`WORKTREE=1` for uncommitted changes) |
| `make deploy <host>` | Build here, and install the same on an ssh host through `doas rsync` (needs rsync and doas there), with Teleprinter from `../teleprinter` (else GitHub) as its terminal |
| `make uninstall` | Remove what `make install` put in place |
| `make vm-linux`, `make test-vm`, `make test-vm-rc` | The Linux VM, and its regression tests on this checkout's build and on the `.deb` |
| `make vm` | The FreeBSD test VM: the compositor in a window on dwl |

Building the compositor needs a few system libraries (xkbcommon, pixman, udev, libinput,
libseat, GBM, EGL, libdrm); `make deps` lists the packages. Design decisions are in
[docs/adr](docs/adr/README.md).

## Licence

BSD-3-Clause, like Patina.
