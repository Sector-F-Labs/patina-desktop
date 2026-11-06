# 0010. Move the Notebook skin and bar_controls into Patina

Date: 2026-10-11

## Status

Accepted: the user asked for the Notebook, Calendar and Steam Classic skins to move into Patina's
core so every app offers them. Supersedes the parts of ADR-0009 that put the skin file and
`bar_controls` in this app.

## Context

Notebook compiled in its own skin and its own `bar_controls`, and the other app (Calendar or
Notebook) had an identical copy of `bar_controls`. The user wanted every Patina app to offer the
app skins.

## Decision

- The skin file moves to Patina's `skins/notebook.toml`, built in from patina-ui 0.1.2 (Patina's
  ADR-0005). This repo's `skins/` folder is gone.
- `bar_controls` moves into Patina's `TitleBar`, which now colours its controls on any coloured
  bar.
- Notebook still starts in the Notebook skin (`HOME_SKIN`), until the user picks another.

## Consequences

- Notebook offers Default, Calendar, Dark Orange, Notebook and Steam Classic, as every Patina
  app does.
- Changing the Notebook skin's colours is now a Patina change and release.
