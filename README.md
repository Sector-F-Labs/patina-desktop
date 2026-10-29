# patina-launcher

An app launcher in the style of [Patina](https://github.com/Sector-F-Labs/patina): a menu of
every installed app that filters as you type and starts the one you pick. It's the launcher of
the Patina desktop, and runs on its own on any Wayland desktop: on sway, Hyprland, river, KDE
or COSMIC as an overlay, and on GNOME as a centred window, much as wofi is used.

## Using it

The apps are listed by name with their icons. Typing filters them: names starting with what you
typed first, then words of the name, then anything else that matches (generic name, keywords,
comment, the program). Up and Down (or Ctrl+P and Ctrl+N) choose, Enter or a click starts one;
Escape, a click elsewhere or switching away closes it.

Running `patina-launcher` while it's open closes it, so bind one key to it:

- **sway**: `bindsym $mod+d exec patina-launcher`
- **Hyprland**: `bind = SUPER, D, exec, patina-launcher`
- **GNOME**: Settings → Keyboard → Custom Shortcuts, command `/home/you/.local/bin/patina-launcher`
- **[wbindkeys](https://github.com/divanvisagie/wbindkeys)**: `bind("ALT+E", "/home/you/.local/bin/patina-launcher")`

On a layer-shell compositor it opens at the button that opened it, when the bar says where that
is (`PATINA_OPEN_AT`, as patina-bar sets it); otherwise at the bottom-left corner, as a menu from
a bar's button (`PATINA_BAR_EDGE=top` for the top-left). On GNOME, which has no layer shell, it opens centred
over the windows ([ADR-0004](docs/adr/0004-a-centred-window-on-gnome.md)).

Two options change where it opens and where the search box sits:

- `--position bar` (the default) opens at the bar's button; `--position center` opens in the
  middle of the screen, Spotlight style.
- `--search bottom` (the default) puts the search box below the list; `--search top` puts it
  above.

So `patina-launcher --position center --search top` is a Spotlight-style launcher, and a bar's
button can run `patina-launcher --search top` for a top bar.

## Where the apps come from

- **Installed apps**, from the `.desktop` files in every XDG data directory, the user's own first
  (`~/.local/share/applications`, `/usr/share/applications`, Flatpak's and Snap's exports),
  leaving out hidden ones, ones whose `TryExec` is missing, and ones meant only for other
  desktops ([ADR-0001](docs/adr/0001-apps-from-desktop-entries.md)).
- **Folder apps** in `~/Apps`: an app as a folder, as on a Mac, `Name.app` with an `app.toml`
  saying how to run it. Copy one in to install it, delete it to uninstall
  ([ADR-0002](docs/adr/0002-folder-apps-in-apps.md)). They're read by
  [patina-apps](https://github.com/Sector-F-Labs/patina-apps), which
  Files, the Patina file manager, shares
  ([ADR-0003](docs/adr/0003-folder-apps-through-patina-apps.md)).

Apps start in their own process group, so they outlive the launcher; ones that ask for a
terminal get `$TERMINAL`, or foot. Icons come from the icon theme (Adwaita, then hicolor and
pixmaps) or the app's folder, decoded on a thread so the list shows at once.

## Looks

It wears a Patina skin: `PATINA_SKIN` names one, else the Patina bar's
(`~/.config/patina-bar/config.toml`), else the Default skin. In the Patina desktop the bar's
button passes its skin and edge, so the launcher matches the bar and opens from it.

## Install

```sh
make install          # ~/.local/bin/patina-launcher (PREFIX=~/.local)
```

It builds with Rust and needs nothing from the system but a Wayland compositor: it draws in
software ([patina-shell](https://github.com/Sector-F-Labs/patina-shell)). Its Patina
dependencies come from GitHub, not crates.io.

## Develop

```sh
make check            # tests and clippy
make run
```

`src/apps.rs` (.desktop entries, search, starting apps), `src/folders.rs` (folder apps),
`src/icons.rs`, `src/ui.rs` (the list and search box), `src/main.rs` (one copy per display, the
surface). Decisions are in [docs/adr](docs/adr/README.md).
