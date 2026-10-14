# patina-bar

A status bar for [dwl](https://codeberg.org/dwl/dwl), in the style of
[Patina](https://github.com/Sector-F-Labs/patina): egui on a wlr-layer-shell surface, skinned
with the same skins as the Patina apps. It is the first piece of a Patina desktop.

![patina-bar on dwl](docs/screenshot.png)

## Running

Make it dwl's status command, so it reads dwl's status from stdin:

```sh
dwl -s patina-bar
```

It puts a bar on every output, showing that output's tags, layout and focused window. Tags are
dwm-style: the viewed ones are in the selection colour, a hollow square marks an occupied tag
and a filled one the focused window's, and urgent ones are in the error colour.

## Config

`~/.config/patina-bar/config.toml`, or `patina-bar -c <file>`. Every key is optional:

```toml
position = "top"            # or "bottom"
height = 26
skin = "Default"            # a Patina skin, or one in ~/.config/patina-bar/skins
tags = ["1", "2", "3", "4", "5", "6", "7", "8", "9"]
hide_vacant_tags = false
left = ["tags", "layout"]
center = ["title"]
right = ["load", "clock"]
clock = "%a %-d %b  %H:%M"  # strftime, as in date(1)
```

The modules are `tags`, `layout`, `title`, `appid`, `clock` and `load`. Unknown keys and modules
are reported on stderr and skipped. A broken file falls back to the defaults.

## Limits

- Tags can't be clicked yet. dwl's status pipe is one-way (see
  [ADR-0004](docs/adr/0004-dwl-status-from-stdin.md)).
- It needs a compositor with wlr-layer-shell, so it won't run on GNOME.

## Build

`make` lists the targets:

| Target | |
|---|---|
| `make build` | Release binary |
| `make check` | Tests and clippy |
| `make install` | Binary into `~/.local/bin` |
| `make vm` | Boot the FreeBSD test VM: dwl with the bar ([bsdt](https://bsdt.divanv.com)) |
| `make vm-restart` | Rebuild in the VM and restart dwl |
| `make vm-gui` | Open the VM's desktop over VNC |
| `make screenshot` | The VM's bar to `docs/screenshot.png` |

Design decisions are in [docs/adr](docs/adr/README.md).

## Licence

BSD-3-Clause, like Patina.
