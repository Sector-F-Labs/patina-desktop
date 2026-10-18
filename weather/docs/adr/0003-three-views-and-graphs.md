# 0003. Three views: Overview, Week and Day, with graphs in Week and Day

Date: 2026-10-18

## Status

Proposed

## Context

The user asked for macOS Weather's features, but with egui's graphs. They want to see a whole
week, click any day for its details, and see graphs of the temperature through the day and of
precipitation.

macOS Weather has one scrolling page: now, hourly, 10-day and tiles. A sheet then opens per
day, with one graph at a time and a menu to choose between Conditions, Precipitation, Wind and
the rest. On a desktop with room to spare, graphs side by side are more useful than one at a time
behind a menu.

## Decision

We will have three views, switched from the right of the header bar (Patina's place for the app's
main output) and with Ctrl+1/2/3 or O/W/D:

- **Overview:** macOS Weather's page. The weather now, the next 24 hours (with sunrise and
  sunset among them), the next 7, 10 or 14 days with range bars, and tiles for feels-like, UV,
  wind, precipitation, humidity, visibility, pressure, the sun and air quality.
- **Week:** the days side by side, then graphs across all of them: temperature (with the
  weather along the top and each day's high and low), chance and amount of precipitation, and
  wind.
- **Day:** a strip for picking the day, a summary, then a graph for each kind of weather over its
  hours: temperature and feels-like, chance of precipitation, precipitation (rain and snow
  stacked), wind with direction arrows, UV, humidity, cloud cover, pressure and visibility.

Clicking a day anywhere (a forecast row, a week card, a point on a week graph) opens it in Day.
Graphs over the same hours share their x axis and a linked hover cursor, shade the nights from
sunrise and sunset, and mark now. Zooming and panning are off, so the graphs read like figures
rather than tools.

## Consequences

- Every requested view is one click or key away, and the graphs show more at once than macOS
  Weather does.
- There's more on screen to keep consistent, and three views to keep working at small widths.
  The overview and day views drop to one column under about 860 and 760 px.
- Everything drawn in the views comes from the skin or the visuals, via `palette.rs`.
