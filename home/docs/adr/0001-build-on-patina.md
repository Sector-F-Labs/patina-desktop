# 0001. Build Home as a native egui app on Patina

Date: 2026-10-28

## Status

Accepted

## Context

Home is a private desktop interface for the Home Assistant instance, in the Sector F Labs suite.
Home Assistant's own web UI and companion apps already exist, but they don't look or behave like
the rest of the user's Patina apps (Calendar, Notebook, Photograph), can't take Patina skins, and
carry far more than the user wants day to day. A native egui app follows the same pattern as
Calendar, which is also a client for a self-hosted server. A web wrapper and a GTK app were not
seriously considered: the suite is Patina.

## Decision

We will build Home with egui/eframe on [Patina](https://github.com/Sector-F-Labs/patina)
(`patina-ui`):

- Patina's own window frame, with the app's controls in its header bar and a "System Title Bar"
  setting as the escape hatch.
- The desktop's light/dark setting followed through `patina::appearance`.
- Skins in the suite format through `patina::skin`: Default (stock egui), Patina's built-ins such
  as Dark Orange, and the shared `~/.config/patina/skins/`. The app's own skin keys go under
  `[app.home]`.
- Stock egui style and fonts otherwise, per Patina's design language.

## Consequences

- The app looks and behaves like the rest of the Patina family and takes the same skins.
- It moves to a new egui version only together with Patina.
- (Anything else this commits the app to.)
- Patina comes from its `develop` branch on git, not crates.io, as for every Sector F app.
