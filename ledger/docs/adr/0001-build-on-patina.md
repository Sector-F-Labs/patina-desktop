# 0001. Build Ledger as a native egui app on Patina

Date: 2026-11-05

## Status

Accepted

## Context

Ledger is for bookkeeping: tabbed spreadsheets of income, expenses and totals, kept in files the
user owns. It sits beside the other Sector F desktop apps (Notebook, Calendar, Photograph), which
are native egui apps on Patina. The user asked for it as a Patina app. Nothing else was
seriously considered: a web app would need a server or a browser sandbox for file access, and a
spreadsheet in another toolkit would not share Patina's frame, skins and tabs.

## Decision

We will build Ledger with egui/eframe on [Patina](https://github.com/Sector-F-Labs/patina)
(`patina-ui`), through its `develop` branch like every Sector F app:

- Patina's own window frame, with a "System Title Bar" setting as the escape hatch.
- The sheets are Patina's `TabStrip` in the title bar, after the menus (Patina ADR-0009).
- The desktop's light/dark setting followed through `patina::appearance`.
- Skins in the suite format through `patina::skin`, with the app's own folder `~/.config/ledger/skins/`.
  The app's own skin keys would go under `[app.ledger]`; it has none yet. The starting skin is
  Default, stock egui following the desktop.
- Stock egui style and fonts otherwise, per Patina's design language.

## Consequences

- Ledger looks and behaves like the rest of the Patina family and takes the same skins.
- It moves to a new egui version only together with Patina.
- The grid is drawn by the app (egui has no spreadsheet widget). It takes its colours from the
  active `Visuals`, so skins reach it. If a second app needs a grid, it moves into Patina
  (Patina ADR-0001).
