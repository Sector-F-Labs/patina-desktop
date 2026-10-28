# 0006. Graph numeric sensors with history from the REST API

Date: 2026-10-28

## Status

Accepted

## Context

The user wants temperatures and wattages graphed, like the weather app's graphs. Home Assistant's
own sensor cards show a graph from recorded history. History is available over REST
(`/api/history/period`, with `minimal_response` and `no_attributes` to keep it small) and over the
WebSocket API. The weather app draws its graphs with `egui_plot` (its ADR-0003): a line coloured along
a temperature scale with a soft fill, highs and lows marked, hours along the bottom, and no zooming
or panning.

## Decision

We will graph every sensor that has a number and a unit (`chart::is_graphable`), in the weather
app's style, with `egui_plot`:

- Cards on a dashboard (`sensor`, `entity`, `tile`, `gauge`, `thermostat`) show the value with the
  past 24 hours under it. Clicking the name or the graph opens a window with the big graph, a choice of
  6 hours, 24 hours or 7 days, min/average/max, the highs and lows marked, and the value under the
  cursor.
- Temperature is coloured along the weather app's scale; power, humidity and other sensors have
  their own colours. A skin can change them in `[app.home]` (`src/palette.rs`).
- History is fetched over REST, on a thread per request, for just the sensors a frame drew, and
  again after a minute (`history::Store`). The current value is added as the last point, and long
  series are averaged down to 400 points.

## Consequences

- Graphs cost nothing for sensors that aren't on screen, and need no changes to the polling of
  [0003](0003-poll-rest-on-a-worker-thread.md).
- The graph stays up to a minute behind between fetches, but its last point is always the current
  value.
- Averaging into buckets hides short spikes in a 7-day graph.
- Counters like energy (`kWh`) are drawn as plain lines, not as bars per period.
- Long-term statistics (months) aren't used; the history API only goes as far as the recorder keeps.
- Only sensors are graphed so far: a light's brightness over time, binary sensors' on/off bars
  and climate entities aren't.
