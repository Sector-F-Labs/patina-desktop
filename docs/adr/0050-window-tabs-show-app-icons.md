# 0050. The bar's window tabs show app icons, and Settings has a Bar page

Date: 2026-10-17

## Status

Proposed. The user asked for icons in the bar's window list, with a choice of icons only,
titles only, or both, and for that choice to live in a new Bar page in Settings. Where the
icons come from and the page's other settings are my design.

## Context

The bar's tabs are the window list (ADR-0012, ADR-0027), and they showed only titles. Titles
are long and alike ("~" in three terminals), and an app's icon is quicker to recognise. With
many windows open, icon-only tabs fit far more.

The bar's settings lived only in its config file. Settings could only open that file, from a
button on the Appearance page.

Icons:

- Windows say what they are with a Wayland app id or an X11 class. Neither is an icon. The
  freedesktop way is to find the app's `.desktop` entry and look up its `Icon=` in the icon
  theme. The launcher already does that lookup (Adwaita, then hicolor, then pixmaps) and draws
  with patina-apps' SVG/PNG renderer, so the bar follows it.
- App ids don't always match the entry's id. GNOME apps use reverse-DNS ids, X11 apps give a
  class like `Steam`, and some only match through `StartupWMClass`.
- The tab strip is Patina's `TabStrip`, so icons belong there (Patina's ADR-0018). The bar
  supplies the textures.

## Decision

- **Patina's `TabStrip`** gains `.icons()` and `.label(TabLabel)`. With an icon and a title, the
  icon comes before the title. With the icon alone, the tab is 44 px wide and shows the title
  on hover. While an icon is missing, the title's first letter stands in for it.
- **The bar** has a new `window_tabs` key: `"both"` (the default), `"icons"` or `"titles"`.
  `icons.rs` matches an app id to an entry case-insensitively, by the entry's id, then by its
  `StartupWMClass`, then by the last part of its id (`nautilus` for `org.gnome.Nautilus`).
  Without a match, it tries an icon named after the app id. The lookup runs on a thread, once
  per app id, at the screen's scale, and the entries are read again when an app id isn't among
  them. Each output's egui context gets its own textures.
- **The bar follows its config live** for `window_tabs` and `position`, as well as `skin`. A
  new edge moves the layer surface (patina-shell's `Host::set_anchor`), and the compositor
  moves the work area with it. `height` and the module lists still need the bar to restart.
- **Settings has a Bar page** between Appearance and Wallpaper. It has the window tabs (Icons and
  titles, Icons, Titles), the position (bottom or top), and "Open bar settings" for the rest.
  That button moved here from Appearance so it isn't in two places (ADR-0027).
  `patina-settings --page bar` opens it.

## Consequences

- Windows are recognisable at a glance, and icon-only tabs leave room for many.
- Icon-only tabs have no close cross. A middle click, the tab's menu, or Super+W closes them.
- An app with no entry and no icon named after its app id gets a letter. Folder apps (ADR-0035)
  are found only if they install an entry.
- The bar now depends on freedesktop-desktop-entry, freedesktop-icons and patina-apps, as the
  launcher does.
