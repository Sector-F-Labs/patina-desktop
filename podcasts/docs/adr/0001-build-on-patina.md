# 0001. Build Podcasts as a native egui app on Patina

Date: 2026-10-23

## Status

Accepted

## Context

Podcasts is a podcast player for the Linux desktop. It began with a question: is a podcast like
Coffee Break Swedish, linked from Apple Podcasts, open enough to build your own client for? It
is. Apple's lookup API gives the show's public RSS feed, and the feed links each episode's MP3.
The user then asked for an app called Podcasts built with Patina.

No other toolkit was considered. The user asked for Patina, and the rest of the family (Calendar,
Notebook, Photograph, UnAmp, Weather) is built on it.

## Decision

We will build Podcasts with egui/eframe on [Patina](https://github.com/Sector-F-Labs/patina)
(`patina-ui`, from its `develop` branch, like every Sector F app):

- Patina's own window frame, with word menus on the left of the header bar (File, Playback, View,
  Skins) and a "System title bar" setting as the escape hatch. A player is a consumption app, like
  UnAmp, so the right of the header bar stays empty. The playback controls go in a player bar
  above the status bar.
- The desktop's light/dark setting followed through `patina::appearance`.
- Skins in the suite format through `patina::skin`, from the built-ins and
  `~/.config/podcasts/skins/`. The app paints no colours of its own yet, so it has no
  `[app.podcasts]` section.
- No home skin: Podcasts starts in Default (stock egui), like UnAmp and Weather.
- Stock egui style and fonts otherwise, per Patina's design language. Artwork is drawn with
  `egui_extras`' image loaders from thumbnails the app caches.
- Audio through rodio with its Symphonia decoders, as in UnAmp.

## Consequences

- The app looks and behaves like the rest of the Patina family and takes the same skins.
- It moves to a new egui version only together with Patina, and `egui_extras` must move with it.
