# 0079. Default apps are chosen in Patina's own defaults.toml, from a Settings page

Date: 2026-10-22

## Status

Accepted. The user asked for a Settings page for default apps (the browser, the image app and so
on), covering every kind of file Files opens, and for the choices to be Patina's own, in TOML, so
GNOME's are left alone.

## Context

Files and every Patina app open a file or link with `xdg-open` (patina-ui's `open::path`), which
takes the desktop's default app for its type. The session supplies its own fallbacks in the
installed `patina-mimeapps.list` (ADR-0066), but the only ways to change a default were an app's
Open With, or `xdg-mime default` by hand. Nothing in the desktop showed which app opens what.

xdg-open and GLib look for a default in this order: the config folders' lists, with
`<desktop>-mimeapps.list` before `mimeapps.list` in each, then the data folders' `applications/`
lists, and only after those `mimeinfo.cache`, which lists every app that takes the type. A
`<desktop>-mimeapps.list` is read only when its desktop is in `XDG_CURRENT_DESKTOP`, which is
`Patina` in this session.

Considered and ruled out:

- **Writing the user's `~/.config/mimeapps.list`,** as Open With does. Tried first. GNOME reads
  that file as well, so a browser chosen in Patina would change GNOME's browser too.
- **Only a TOML file that a Patina opener reads.** Patina's apps would follow it, but GTK and Qt
  apps, the portal and terminal programs only know xdg-open's lists.
- **One line per MIME type.** Hundreds of rows, most of which nobody changes.

## Decision

- **The choices live in `~/.config/patina/defaults.toml`,** a role a line, by desktop file id:
  `browser = "firefox"`, `images = "org.gnome.Loupe"`. The roles are browser, email, calendar,
  folders, images, documents, text, music, video and archives.
- **`patina_system::defaults::sync` writes them out as `~/.config/patina-mimeapps.list`,** for
  each role's first type and for each of its other types the chosen app claims. xdg-open and GLib
  read that file only in Patina. It starts with a marker line, and a list of that name without
  the marker (one written by hand) is left alone and reported. With no choices, the file is
  removed.
- **Settings syncs on every choice, and the compositor syncs at start-up** on a thread, so a hand
  edit to the TOML applies from the next session.
- **The Defaults page** shows one row per role: a drop-down of the installed apps whose desktop
  entry claims the role's first type (`image/png` for Images, `x-scheme-handler/https` for the
  browser). The app shown is the TOML's choice, else what xdg-open would find in this session.
- **The roles cover the installed `patina-mimeapps.list`:** every type that file gives a default
  for belongs to a role, and a test fails if one is added there without one.

## Consequences

- The browser, image viewer and the rest can be chosen in one place, and in Patina Files, the
  bar's links and GTK and Qt apps all follow, since they go through xdg-open or GLib. GNOME
  sessions keep GNOME's defaults.
- In Patina, a Settings choice wins over an Open With choice for the same type, because
  `patina-mimeapps.list` is read before `mimeapps.list`. Open With still decides types that no
  role covers.
- `~/.config/patina-mimeapps.list` is generated, and edits to it are overwritten. The TOML is the
  file to edit.
- There's no separate "editor" role. MIME defaults have one opener per type, so the image app
  that opens a PNG on a double click is the one chosen under Images.
- Types outside the roles still use the installed list or `mimeinfo.cache`. A role can be added
  when one matters.
- The page reads the installed apps once, when it's first shown. An app installed while Settings
  is open appears the next time Settings is opened.
- The terminal isn't a MIME default. It's still `TERMINAL`, set by the compositor (`clients.rs`),
  and could become a `terminal` key here later.
