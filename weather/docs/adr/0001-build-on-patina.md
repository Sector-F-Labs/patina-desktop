# 0001. Build Weather as a native egui app on Patina

Date: 2026-10-18

## Status

Accepted

## Context

Weather is a desktop weather app modelled on macOS Weather's features: the weather now, the next
hours, the days ahead, and details such as UV, wind and pressure. It was asked for as a Patina
app, partly because egui gives graphs and custom painting cheaply. macOS Weather keeps its graphs
behind a day sheet; here they can be a first-class view.

No other toolkit was considered. The user asked for Patina, and the rest of the family (Calendar,
Notebook, Photograph, UnAmp) is built on it.

## Decision

We will build Weather with egui/eframe on [Patina](https://github.com/Sector-F-Labs/patina)
(`patina-ui`):

- Patina's own window frame, with word menus on the left of the header bar (File, View, Units,
  Skins) and the view switch (Overview, Week, Day) on the right, plus a "System title bar"
  setting as the escape hatch.
- The desktop's light/dark setting followed through `patina::appearance`.
- Skins in the suite format through `patina::skin`, from the built-ins and `~/.config/weather/skins/`.
  The app's own colours (weather icons, the temperature scale, graph colours) go under
  `[app.weather]`.
- No home skin: Weather starts in Default (stock egui), like UnAmp.
- Stock egui style and fonts otherwise, per Patina's design language.
- Graphs with `egui_plot`, the egui project's plotting crate, at the version built for Patina's egui.

## Consequences

- The app looks and behaves like the rest of the Patina family and takes the same skins.
- It moves to a new egui version only together with Patina, and `egui_plot` must have a release
  for that egui version too (`egui_plot` 0.36 is the one for egui 0.35).
