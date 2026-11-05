# 0104. Your picture is `~/.face`, chosen on Settings' Account page

Date: 2026-11-05

## Status

Accepted

## Context

ADR-0028 left the lock screen with no avatar. The password box was reworked to look like GNOME's
and KDE's login cards, which put the user's picture above their name, and that needs somewhere
for the picture to come from and a way to choose it. GNOME and KDE both read `~/.face`, and
AccountsService keeps a copy for login managers, which can't read home folders. A full user manager
(creating users, passwords, admin rights) would need root or polkit prompts and is far more than a
picture needs.

## Decision

- The picture is `~/.face`, a 256 pixel square PNG. `patina_system::avatar` reads and writes it,
  cropping any image to a square from the middle.
- Settings gets an **Account** page that sets it: the Sector F flask in each skin's colours (as the
  default wallpaper is the flask in the skin's), any image in `~/Pictures/Avatars`, any picture through the file chooser portal (Files' in the
  Patina session, as `rfd` asks for it), or none.
- Saving also calls AccountsService's `SetIconFile` for the user, best effort: without the service,
  or without permission, `~/.face` alone is written.
- patina-lock draws the picture in a circle above the name, and the first letter of the name in a
  disc where there's none.
- The page edits only the running user's picture. No user manager.

## Consequences

- The lock screen and, through AccountsService, a login manager show the same picture; so do GNOME
  and KDE apps that read `~/.face`.
- The flask pictures are made when chosen, so changing the skin afterwards doesn't recolour one.
- Real names and passwords are still changed outside Patina (`chfn`, `passwd`).
- Pictures kept in `~/Pictures/Avatars` show as tiles, as wallpapers do from
  `~/Pictures/Wallpapers`; "Choose a picture…" reaches the rest. Without a portal chooser
  installed the button does nothing.
