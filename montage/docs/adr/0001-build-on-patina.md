# 0001. Build Montage as a native egui app on Patina

Date: 2026-10-26

## Status

Accepted

## Context

Montage is a video editor for home movies, in the spirit of iMovie and Windows Movie Maker rather
than Premiere or DaVinci Resolve: put clips in a library, drag them onto a timeline, and lay a
music track and a voiceover under them. It's meant for the Patina desktop (`patina-desktop`),
where the other apps are Patina apps, but lives in a repository of its own.

The Patina desktop has no video editor, and the ones that run there (Kdenlive, Shotcut,
OpenShot, Pitivi) are Qt or GTK apps aimed further up, at editors, that don't share the
desktop's frame, menus or skins. A web app was never on the table: everything it edits is local
files, played in real time.

## Decision

We will build Montage with egui/eframe on [Patina](https://github.com/Sector-F-Labs/patina)
(`patina-ui`, from its `develop` branch, as every Sector F app takes it):

- Patina's own window frame: the menus (File, Edit, View, Playback, Skins) on the left of the
  header bar, and Export, the app's output, on the right. A "System Title Bar" setting is the
  escape hatch.
- The desktop's light/dark setting followed through `patina::appearance`.
- Skins in the suite format through `patina::skin`: Default (stock egui, where Montage starts),
  Patina's built-ins, and `~/.config/montage/skins/`. The timeline's own colours (`video`,
  `music`, `voiceover`, `waveform`, `playhead`) go under `[app.montage]`.
- Stock egui style and fonts otherwise. The timeline and library tiles are painted, since egui
  has no such widgets, in colours from the visuals or the skin.
- A `make app` target that makes a `Montage.app` folder app for the Patina desktop's `~/Apps`,
  beside the usual per-user `make install`.

## Consequences

- Montage looks and behaves like the rest of the Patina family and takes the same skins.
- It moves to a new egui version only together with Patina.
- The timeline is Montage's own painting and pointer handling, tested by feeding egui pointer
  events, rather than a toolkit widget.
