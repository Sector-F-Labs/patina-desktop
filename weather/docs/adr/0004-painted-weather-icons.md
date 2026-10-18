# 0004. Paint the weather icons from shapes

Date: 2026-10-18

## Status

Proposed

## Context

Weather icons appear everywhere: the hero, the hourly strip, the day rows, the week cards and
along the top of the temperature graphs, at sizes from 18 to 72 px. The options were:

- **Emoji** from egui's bundled emoji font. They're inconsistent, partly missing (there's no
  moon behind a cloud), and their colours can't be skinned.
- **Bundled SVG or PNG icon set** (e.g. Meteocons). They look polished, but the colours are fixed
  in the files, contrary to Patina's "skinnable all the way down". They'd also need a licence
  check and the `egui_extras` image loaders.
- **Painting them with egui's painter** from circles, lines and a few polygons.

## Decision

We will paint the icons in `icons.rs`, from the WMO weather code and whether it's day. They are a
sun or crescent moon, clouds, rain streaks, snowflakes, fog bars and a lightning bolt, combined.
Their colours (sun, moon, cloud, rain, snow, lightning) come from the palette, which a skin can
change under `[app.weather]`.

## Consequences

- The icons are crisp at any size, follow light and dark, and are skinnable.
- They're simpler than a designed icon set. Making them prettier means editing drawing code
  rather than swapping files.
- egui fills only convex shapes, so the crescent moon is built as a triangle strip and the bolt
  from two convex halves.
