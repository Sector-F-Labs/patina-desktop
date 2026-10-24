# 0087. Settings sets light or dark, through GNOME's color-scheme

Date: 2026-10-24

## Status

Accepted. The user asked for a way in Settings to switch between dark and light mode.

## Context

Apps learn the desktop's preference from xdg-desktop-portal's `org.freedesktop.appearance
color-scheme`: Patina apps on the Default skin (`patina::appearance`), GTK 4 and libadwaita,
Firefox, Qt 6. The portal's GNOME and GTK backends serve it from GNOME's `org.gnome.desktop.interface
color-scheme`, which the icon theme beside it is already set through (`icons.rs`). The shell
itself (bar, menus, lock screen) wears a skin, and every skin but Default fixes light or dark.

## Decision

- **A Style row on the Appearance page** (`settings/src/color_scheme.rs`): Default, Light and
  Dark, written with `gsettings set org.gnome.desktop.interface color-scheme` as `default`,
  `prefer-light` and `prefer-dark`. Apps that listen switch at once.
- **The skin is left alone.** A skin is the shell's look and keeps its own base; the style is
  what apps are asked to be. Where gsettings or the key is missing, the row says so.

## Consequences

- GTK 3 apps don't follow `color-scheme`; they need a dark GTK theme (Yaru-dark), not set here.
- Without a portal backend reading GNOME's settings, apps don't hear of a change.
- It's GNOME's key, as the icon theme is; a setting of the desktop's own (served by its own
  portal backend) can take its place later.

## Revisit when

- Patina serves the appearance portal itself, or the shell's skins come in light and dark pairs
  that could follow the style.
