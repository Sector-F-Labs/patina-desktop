# Patina desktop

The first pieces of a desktop in the style of [Patina](https://github.com/Sector-F-Labs/patina):

- **patina-compositor** (`compositor/`): a floating Wayland compositor built on
  [Smithay](https://github.com/Smithay/smithay). For now it runs as a window on another desktop,
  drawn with OpenGL ES (apps hand it GPU buffers through linux-dmabuf), or in software where
  there's no GPU (`PATINA_RENDERER=pixman` forces that). It starts patina-bar inside it.
- **patina-bar** (`bar/`): a status bar in egui on a wlr-layer-shell surface, skinned with the
  same skins as the Patina apps. It runs on any layer-shell compositor, including dwl, where it
  also shows dwl's tags.
- **patina-launcher** (`launcher/`): opened by the bar's Sector F button. It lists the installed
  apps from their `.desktop` files, with icons, and filters them as you type. Up and Down choose,
  Enter or a click starts one, and Escape, clicking elsewhere or the button again closes it.
- **patina-volume** (`volume/`): the volume menu, opened by the bar's speaker: mute, a slider and
  the level. It also fades in briefly when the volume keys are used.
- **patina-wifi** (`wifi/`): the Wi-Fi menu, opened by the bar's Wi-Fi icon: networks in range,
  joining (with a password box for new ones), disconnecting and Wi-Fi on and off, through
  NetworkManager.
- **patina-lock** (`lock/`): the lock screen, in the bar's skin: the time, and a password box
  checked through PAM. The compositor runs it on Ctrl+Super+Q, Super+Escape and before sleep.
- **patina-shell** (`shell/`): what the bar and launcher share, a layer-shell host for egui
  drawn in software.

![patina-compositor running foot and patina-bar](docs/screenshot.png)

## Running

```sh
make run                          # the compositor in a window, with the bar
patina-compositor foot firefox    # and any clients to start besides the bar
dwl -s patina-bar                 # the bar alone, on dwl, with dwl's tags
```

The compositor needs a Wayland desktop to open its window on. On GNOME the window has no title
bar; move it with Super+drag. Windows inside it float and cascade, and a click focuses and raises
one.

## The bar's config

`~/.config/patina-bar/config.toml`, or `patina-bar -c <file>`. Every key is optional:

```toml
position = "bottom"         # or "top"
height = 41                 # as tall as a Patina title bar
skin = "Default"            # a Patina skin, or one in ~/.config/patina-bar/skins
tags = ["1", "2", "3", "4", "5", "6", "7", "8", "9"]
hide_vacant_tags = false
left = ["launcher", "windows", "tags", "layout"]
center = ["title"]
right = ["tray", "clock"]
clock = "%a %-d %b  %H:%M"  # strftime, as in date(1)
launcher = "patina-launcher"  # what the Sector F button runs, through sh(1)
```

The modules are `launcher` (the Sector F flask, which runs `launcher` when clicked), `windows`
(the open windows as Patina tabs: a click brings one forward, its cross closes it), `tags`,
`layout`, `title`, `appid`, `clock` and `tray` (a Wi-Fi icon showing the signal, a
speaker showing the volume and, on laptops, the battery; clicks open the Wi-Fi and volume menus, run by
`wifi = "patina-wifi"` and `volume = "patina-volume"`). `windows` needs a compositor with
wlr-foreign-toplevel-management, such as patina-compositor or sway. `tags` and `layout` come
from dwl and show nothing elsewhere. Unknown keys and modules are reported on stderr and skipped.
A broken file falls back to the defaults.

## Not yet

- Minimise and fullscreen.
- Workspaces.
- More than one monitor, hotplug and the lid on the hardware. Running it as
  your session (one monitor, from a text console or the login screen) is in
  [docs/running-as-a-session.md](docs/running-as-a-session.md).

## Build

`make` lists the targets:

| Target | |
|---|---|
| `make run` | The compositor in a window on this desktop |
| `make check` | Tests and clippy |
| `make install` | The binaries into `~/.local/bin` |
| `make install-session` | Also add Patina to the login screen ([how](docs/running-as-a-session.md)) |
| `make vm` | The FreeBSD test VM: the compositor in a window on dwl ([bsdt](https://bsdt.divanv.com)) |
| `make vm-restart` | Rebuild in the VM and restart its desktop |
| `make vm-gui` | Open the VM's desktop over VNC |
| `make screenshot` | The VM's desktop to `docs/screenshot.png` |

Building the compositor needs a few system libraries (xkbcommon, pixman, udev, libinput,
libseat, GBM, EGL, libdrm); `make deps` lists the packages. Design decisions are in
[docs/adr](docs/adr/README.md).

## Licence

BSD-3-Clause, like Patina.
