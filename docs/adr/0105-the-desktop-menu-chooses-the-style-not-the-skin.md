# 0105. The desktop's menu chooses the style, not the skin

Date: 2026-11-05

## Status

Accepted

Supersedes the skin list in [0029](0029-desktop-menu-chooses-the-skin.md).

## Context

ADR-0029 put the shell's skins on the right click menu so a skin could be changed from the
desktop. Settings' Appearance page has since taken that over (ADR-0038), and a skin is rarely
changed in a hurry. What is changed more often is light or dark, which is the one choice that
applies to every app (ADR-0087).

## Decision

- The desktop's menu no longer lists the skins. Settings' Appearance page chooses the skin.
- It offers the style instead: Default, Light and Dark, GNOME's `color-scheme` set through
  `gsettings`, as Settings' Appearance page does. The code is shared in
  `patina_system::color_scheme`.
- The items that copy the built-in skins, open the skins folder and open the bar's settings file
  stay.

## Consequences

- The menu is shorter, and its first choice is the one people switch most.
- Style changes apply to apps on the Default skin and to those that follow the system (GTK 4,
  Firefox); a skin with its own colours doesn't change with it, as before.
- Without gsettings, the menu says the style isn't set up on this system.
