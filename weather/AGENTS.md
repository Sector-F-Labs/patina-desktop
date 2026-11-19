# Weather

A Patina (egui) weather app after macOS Weather: Overview, Week and Day views, graphs with
`egui_plot`, forecasts and place search from Open-Meteo (no key).

Weather is a member of the patina-desktop workspace (one `Cargo.lock` and `target/` for the
repository; build it with `-p weather`, which the Makefile does), and still its own app. It used to
be its own repository, Sector-F-Labs/weather; the history up to the move is kept here.

Significant decisions are ADRs in `docs/adr/`; read them before changing the data source, the
views or the icons. Use the `adr` skill to add one.

## Commands

```bash
make check         # tests + clippy -D warnings
make dev           # run in a scratch config seeded from packaging/dev-config.toml
make screenshot    # docs/screenshot-*.png, in a scratch config
make icons         # assets/weather-icon-128.png from packaging/linux/weather.svg
make install       # per-user install; test with PREFIX=<scratch dir>
```

Never run test instances against the user's config: the Makefile's `SCRATCH_ENV` points XDG
config, data and cache at `target/scratch`. eframe's persistence keeps the window size in the
data dir, so delete `target/scratch/data` to try a different window size. In zsh, an unquoted
`$VAR` holding several `A=b` words doesn't split, so run such commands through `bash -c` or make.

Screenshot mode: `WEATHER_SCREENSHOT=path.png`, plus `WEATHER_VIEW=overview|week|day`,
`WEATHER_DAY=<days from today>` and `WEATHER_SKIN=<name>`.

## Layout

- `src/forecast.rs`: `Place`, `Forecast` (current, hours, days, air), Open-Meteo JSON parsing,
  weather codes. Times are the place's local time; `Forecast::now()` uses its UTC offset.
- `src/fetch.rs`: requests on short-lived threads with replies over a channel; the cache.
- `src/units.rs`: display units. Data stays metric.
- `src/palette.rs`: every colour the app paints, from defaults and the skin's `[app.weather]`.
- `src/icons.rs`: weather icons painted from shapes (ADR-0004).
- `src/widgets.rs`: `Scene`, `Action`, cards, range bar, gauges, and a test that draws every view.
- `src/charts.rs`: `Range` (hours, linked cursor, night shading, hover label) and the graphs.
- `src/overview.rs`, `src/week.rs`, `src/day.rs`: the three views; they return `Action`s.
- `src/app.rs`: window, header and menus, sidebar and search, status bar, shortcuts, screenshot mode.
- `src/config.rs`: `~/.config/weather/config.toml`.

`testdata/stockholm.json` is a real 14-day response for the tests. The views must draw whatever
today's date is, since tests run long after it was fetched.
