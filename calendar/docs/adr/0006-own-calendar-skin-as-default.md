# 0006. Ship a built-in Calendar skin, red bar on paper, and start in it

Date: 2026-10-11

## Status

The skin itself is Accepted: the user asked for a built-in skin called Calendar, with a red title
bar and the rest paper white, like the icon. Making it the starting skin, and the way the bar's
controls are coloured, are Proposed.

## Context

Patina apps start in Default (stock egui) and offer Dark Orange plus the shared skins folder. The
user wanted Calendar to come with a look of its own, taken from its icon: a red-orange header on
an off-white page.

With a coloured title bar, the bar's menus and buttons would still be drawn in the skin's dark
text colours, so they wouldn't read on red. Patina's `TitleBar` has no way to colour its controls
from the `[window]` section yet.

## Decision

- **The skin:** `skins/calendar.toml`, compiled in alongside Patina's built-ins:
  - **Bar:** the icon's header colour, `#D9481A`, with white text and window controls.
  - **Page:** warm off-white panels (`#F7F5F0`), grey ink and the icon's grey for weak text.
  - **Accent:** the icon's orange, for today and the selection.
- **Starting skin:** Calendar is the skin until the user picks another from the Skins menu.
  Default and the rest stay one click away.
- **Bar controls:** when a skin's title bar differs from its panel colour, the app colours the
  bar's own controls (`bar_controls` in `app.rs`):
  - text in `title_text`;
  - fills from `[window.controls]` hover and pressed colours, and a translucent wash of the title
    text at rest;
  - the selected view in a stronger wash.

  Popups build their own UI from the window's style, so menus keep the skin's normal colours.
  Skins whose bar matches the panel, such as Default and Dark Orange, are untouched.

## Consequences

- Calendar opens looking like its icon, which departs from Patina's "stock egui first" for this
  app.
- `bar_controls` is useful to any Patina app whose skin colours the title bar. It belongs in
  Patina's `TitleBar`, and moving it there would be a Patina ADR and a patina-ui release.
- Events keep their calendars' own colours on the paper page. The week view's tints mix towards
  the off-white panel, so they come out pastel.
