# 0066. The session has default apps of its own, in patina-mimeapps.list

Date: 2026-10-18

## Status

Accepted. The user found images opening in Firefox from Files, and asked for the fix to live in
the desktop rather than in Files.

## Context

Files, like every Patina app, opens a file with `xdg-open` (patina-ui's `open::path`), so the app
it gets is the desktop's default for the type. On Ubuntu the defaults are in
`/usr/share/applications/gnome-mimeapps.list`, but a `<desktop>-mimeapps.list` is only read when
its name is in `XDG_CURRENT_DESKTOP`, and ours is `Patina`. With no default, xdg-open takes the
first app in `mimeinfo.cache` that claims the type, which is alphabetical: `firefox-nightly`
before `org.gnome.Loupe` for PNG and WebP. JPEG worked only because the user had once chosen
Loupe with Open With.

This is the session's doing, not Files', so it belongs here. Considered and ruled out:

- **`XDG_CURRENT_DESKTOP=Patina:GNOME`,** so GNOME's list is read after ours. It also shows
  `OnlyShowIn=GNOME` entries (GNOME's Settings, Tweaks' extensions) in the launcher and hides
  `NotShowIn=GNOME` ones, and the portal and autostart read it too.
- **Copying `gnome-mimeapps.list` at install.** It only exists on Ubuntu; the FreeBSD VM would
  get nothing.
- **Setting the user's `~/.config/mimeapps.list`.** That's the user's own file, which Open With
  writes, and it applies under GNOME too.

## Decision

`packaging/patina-mimeapps.list` lists, for the common types (folders, images, PDF and EPUB,
plain text, video, audio, archives), the apps to open them in order of preference: Patina's own
first (Files for folders), then the usual GNOME, KDE and Xfce apps and mpv and VLC. xdg-open and
GLib use the first one installed, so one list serves Ubuntu and FreeBSD.

`make install` puts it in `$(PREFIX)/share/applications`, which is `~/.local/share/applications`
for a per-user install, and `make uninstall` removes it. When Patina has an app for a type (an
image viewer, say), it goes at the front of that type's line.

## Consequences

- Images open in Loupe (or eog, gThumb…), PDFs in Papers, folders in Files, in Patina only; GNOME
  sessions keep GNOME's defaults.
- The user's `~/.config/mimeapps.list` still comes first, so an Open With choice wins.
- Types not in the list still fall to `mimeinfo.cache`'s first app, the browser for some. The list
  grows when one turns up.
- The app names are other projects' desktop file ids; one renamed upstream silently drops out
  of its line.
