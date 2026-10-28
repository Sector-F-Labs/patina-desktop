# 0001. Build Draw as a native egui app on Patina

Date: 2026-10-28

## Status

Accepted

## Context

Draw is the Unix MS Paint: a small, immediate bitmap editor for sketching, annotating and
cropping, with the tools everyone knows (pencil, brush, eraser, fill, shapes, colour picker).
Patina desktop has no such app. Considered: wrapping GIMP or Krita (far too heavy, and they
don't look like the desktop) and a web canvas (not native, no file integration). A native
egui app is small enough to start instantly and takes Patina's skins.

## Decision

We will build Draw with egui/eframe on [Patina](https://github.com/Sector-F-Labs/patina)
(`patina-ui`):

- Patina's own window frame, with the app's controls in its header bar and a "System Title Bar"
  setting as the escape hatch.
- The desktop's light/dark setting followed through `patina::appearance`.
- Skins in the suite format through `patina::skin`: Default (stock egui), Patina's built-ins such
  as Dark Orange, and the shared `~/.config/patina/skins/`. The app's own skin keys go under
  `[app.draw]`.
- Stock egui style and fonts otherwise, per Patina's design language.

## Consequences

- The app looks and behaves like the rest of the Patina family and takes the same skins.
- It moves to a new egui version only together with Patina.
- The picture is a CPU bitmap of our own, drawn into an egui texture, so the tools are plain
  Rust that tests cover without a window.
