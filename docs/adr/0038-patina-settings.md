# 0038. A Settings app, patina-settings, for settings that need more than a menu

Date: 2026-10-16

## Status

Accepted

## Context

The desktop's settings live in two TOML files: the bar's config (`skin`, `wallpaper`), which
patina-menu edits (ADR-0029, ADR-0030), and the compositor's config (ADR-0019), which is edited by
hand. ADR-0029 expected that a settings app would come once a menu was no longer enough. Night
Light (ADR-0039) is the first setting that needs one: a mode, two times and a warmth slider don't
fit in a right-click menu. The wallpaper submenu is also cramped: a long list of file names with no
pictures.

The user wanted the right-click menu to keep everything it has now, and its Wallpaper item to open
the Settings app at its wallpaper page instead of showing a submenu. A layer-shell piece like the
menus was ruled out, because Settings is a window you keep open beside others, move and close. That
makes it a Patina app, like Calendar or Notebook.

## Decision

- **patina-settings** is a new crate in the workspace (`settings/`). It's an eframe window with
  Patina's frame, title bar and word menus, in the shell's skin (the bar's `skin`), with app id
  `com.sectorflabs.Settings`.
- It has a sidebar of pages. It starts with **Appearance** (the skin, as patina-menu offers it),
  **Wallpaper** (the default and the images in `~/Pictures/Wallpapers` and the system's backgrounds,
  as thumbnails; the finder moves here from patina-menu) and **Night Light** (ADR-0039). `--page NAME` opens it at a page.
- It edits the files where the settings already live, keeping comments and other keys: the bar's
  config through patina-shell, as patina-menu does, and the compositor's config with `toml_edit`.
  The pieces that read those files watch them, so a change applies straight away. Settings tells no
  one.
- patina-menu keeps its items. Its Wallpaper item runs `patina-settings --page wallpaper`
  instead of opening a submenu, and a new Settings… item opens it at Appearance.
- Like every Patina app it has a status bar (Patina's ADR-0016) for what an action did.

## Consequences

- Settings that don't fit in a menu have somewhere to go. Each new page edits the file its piece
  already reads, so no new settings store comes in.
- The skin appears in both the menu and Settings, which ADR-0027's "don't repeat the UI" argues
  against. The user chose this: the menu is the quick way, and Settings is the full one.
- eframe joins the workspace's dependencies (winit and glow), which the layer-shell pieces didn't
  need.
- Every piece that wants its settings changed live must watch its file. The compositor starts doing
  so with ADR-0039.
