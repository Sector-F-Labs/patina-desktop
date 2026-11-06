# 0008. Install per user with make install, replacing the GTK Notebook's binary

Date: 2026-10-11

## Status

Proposed. The user asked for an icon and a `make install`. Installing per user, and taking over the
`notebook` binary name, are my choices.

## Context

[ADR-0001](0001-rewrite-in-egui-on-patina.md) left out an install target, because the GTK Notebook
installs a binary of the same name to `~/.local/bin/notebook`. The user has now asked for an install
target, along with an icon.

The options were:

- **A `.deb` installed through apt**, as UnAmp does. It needs sudo, packaging scripts and CI.
- **A per-user install into `~/.local`**, as the GTK Notebook did.
- **A different binary name**, to sit beside the GTK version.

## Decision

- **Per-user install:** `make install` builds a release binary and installs everything under
  `PREFIX`, by default `~/.local`:
  - `bin/notebook`;
  - the icon, as `share/icons/hicolor/scalable/apps/com.sectorflabs.Notebook.svg` plus a 128 px
    PNG;
  - the launcher, as `share/applications/com.sectorflabs.Notebook.desktop`, with `Exec` set to the
    absolute path.

  No sudo is needed, and `make uninstall` removes it all.
- **Matching the window:** the launcher and icon are named after the window's app id,
  `com.sectorflabs.Notebook`. GNOME matches windows to launchers by that id.
- **The binary name stays `notebook`**, so installing this version replaces the GTK one's binary.
  This version is the one going forward.
- **The icon:** a black, Moleskine-style hardcover with an elastic band, cream page edges and an
  orange ribbon. It's drawn in `packaging/linux/notebook.svg`. `make icons` renders the window
  icon with the `resvg` crate (`examples/render_icon.rs`), so no system SVG tools are needed.

## Consequences

- **One install command, no sudo**, as before.
- **A leftover GTK launcher:** after installing, the GTK version's `com.divanv.Notebook.desktop`
  still exists, and its absolute `Exec` now starts this version. Run `make uninstall` in the GTK
  repo first, or delete that launcher, so only one Notebook shows in the app grid.
- **No system-wide package.** Add a `.deb` target, as UnAmp has, if Notebook is ever installed
  for other users.
