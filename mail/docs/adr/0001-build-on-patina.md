# 0001. Build Mail as a native egui app on Patina

Date: 2026-10-18

## Status

Accepted

## Context

Mail is a simple desktop mail client for a Proton Mail account, reached through Proton Mail Bridge
running on the same machine. It sits alongside Calendar, Notebook and the other Patina apps, and
should look and behave like them: same frame, same skins, same menus.

The user asked for it to be built with patina-ui. Proton's own web app and general-purpose clients
(Thunderbird, Evolution) already exist; the point here is a small client that belongs to the Patina
family, so they weren't alternatives for this repo.

## Decision

We will build Mail with egui/eframe on [Patina](https://github.com/Sector-F-Labs/patina)
(`patina-ui`):

- Patina's own window frame, with word menus on the left of its header bar and the actions that
  write mail (Reply, Reply All, Forward, New Message) on the right, and a "System title bar"
  setting as the escape hatch.
- The desktop's light/dark setting followed through `patina::appearance`.
- Skins in the suite format through `patina::skin`: Default (stock egui), Patina's built-ins such
  as Dark Orange, and the app's own `~/.config/mail/skins/`. Mail starts in Default; it has no
  skin of its own yet.
- Stock egui style and fonts otherwise, per Patina's design language.

## Consequences

- The app looks and behaves like the rest of the Patina family and takes the same skins.
- It moves to a new egui version only together with Patina.
- Mail is shown as text: HTML-only messages are turned into plain text (html2text), since egui has
  no HTML renderer. Rich newsletters lose their layout and images.
