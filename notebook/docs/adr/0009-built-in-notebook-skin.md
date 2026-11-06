# 0009. Ship a built-in Notebook skin, brown bar on cream paper, and start in it

Date: 2026-10-11

## Status

The skin itself and the brown icon are Accepted: the user asked for a built-in skin called Notebook
in the spirit of Calendar's, brown instead of red, and for the icon to be a brown book rather than
a black one. Making it the starting skin is Proposed.

## Context

Calendar ships its own skin, a red title bar on paper, and starts in it (Calendar's ADR-0006 and
ADR-0007). The user wanted Notebook to have the same kind of look of its own. The icon was a
black Moleskine (ADR-0008), so it changes to brown leather to match.

A coloured title bar needs its menus and buttons recoloured to read on it. Calendar does that with
`bar_controls` in its `app.rs`; Patina's `TitleBar` doesn't do it yet.

## Decision

- **The skin:** `skins/notebook.toml`, compiled in alongside Patina's built-ins:
  - **Bar:** leather brown, `#6E4A2E`, with white text and window controls; hover `#82593A`,
    pressed and window edge `#563820`.
  - **Page:** cream panels (`#F6F1E6`, the icon's pages), sepia ink.
  - **Accent:** the same brown, for the selection.
- **Starting skin:** Notebook is the skin until the user picks another from the Skins menu.
  Default and Dark Orange stay one click away.
- **The icon:** the cover, hinge, edge and elastic band move from black to browns. The cream pages
  and orange ribbon stay.
- **Bar controls:** a copy of Calendar's `bar_controls`, used the same way.

## Consequences

- Notebook opens looking like its icon, a departure from Patina's "stock egui first" for this app,
  as in Calendar.
- `bar_controls` now exists in two apps. Patina's ADR-0001 says that's the point to move it into
  Patina (`TitleBar` colouring its controls from `[window]`), which needs a Patina ADR and a
  patina-ui release. Until then the two copies must be kept the same.
- `make screenshot` now also writes `docs/screenshot-notebook.png`, and the light screenshot asks
  for Default explicitly.
