# 0001. The launcher lists apps from .desktop files

Date: 2026-10-14

## Status

Accepted

## Context

The bar's Sector F button first ran wofi. A Patina launcher needs a list of installed apps. Every
app already describes itself in a freedesktop.org Desktop Entry (`.desktop` file): name, command,
icon, keywords. Distribution packages, `make install` in the Patina apps, Flatpak and Snap all
write them, into the XDG data directories. A list of our own would start empty and need an entry
per app.

The user wants the desktop eventually to treat an `~/Applications` folder full of AppImages the
way macOS treats its Applications folder, with icons handled in an extensible way. That is out of
scope for now. AppImages carry a `.desktop` file inside them, and integration tools such as
AppImageLauncher extract it, so they can join the same list later.

## Decision

patina-launcher lists the entries in every XDG data directory (`freedesktop-desktop-entry`,
without its gettext feature), skipping hidden ones, ones for other desktops (`OnlyShowIn`,
`NotShowIn`; this desktop is `Patina`) and ones whose `TryExec` is missing. Icons come from the
icon theme spec (`freedesktop-icons`, Adwaita then hicolor). Search ranks name prefixes, then
words of the name, then substrings, then the generic name, keywords, comment and program. Usage
history and other launcher state, when they come, live beside the entries, not in them.

## Consequences

- Every installed app is listed with no work from us or the user.
- An AppImage folder is a later, separate source, behind the same list.
- Icons follow the theme spec; reading the user's chosen icon theme (without calling gsettings)
  is still to do.

> Recorded first as patina-desktop ADR-0010, when the launcher lived there.
