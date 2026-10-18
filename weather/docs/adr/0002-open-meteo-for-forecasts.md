# 0002. Take forecasts from Open-Meteo

Date: 2026-10-18

## Status

Proposed

## Context

The app needs current conditions, hourly and daily forecasts for any place, place search, and
ideally air quality. It's a personal open-source app, so an API that needs a key, an account or
payment would make every user sign up somewhere first.

Considered:

- **Open-Meteo:** free for non-commercial use with no key. One request gives current, hourly
  (temperature, feels-like, precipitation and its chance, wind, UV, pressure, visibility, cloud)
  and daily data for up to 16 days, in the place's own time zone. It also has a geocoding API
  for search and an air-quality API. Data is CC BY 4.0, so it asks for attribution.
- **MET Norway (yr.no):** free with no key and very good for the Nordics. But it has no place
  search, no air quality, and its terms ask for careful caching with `If-Modified-Since`.
- **OpenWeatherMap, WeatherKit, Tomorrow.io:** need keys or accounts, and their free tiers are
  limited.

## Decision

We will use Open-Meteo for forecasts, place search and air quality:

- Fetch 14 days with `timezone=auto`, and keep times as the place's local time with its UTC
  offset, so "now" is computed in the place's time rather than the computer's.
- Keep everything in metric and convert for display (`units.rs`). Changing units then needs no
  refetch.
- Requests run on short-lived threads and answer through a channel, never on the UI thread.
- Cache the last forecast of every place in `~/.cache/weather/forecasts.json`, so start-up shows
  data at once. Refetch a place when its forecast is older than 15 minutes.
- Air quality is optional: if that request fails the tile is left out, not the forecast.
- Show "Weather data by Open-Meteo.com" in the status bar, as the licence asks.

## Consequences

- Nobody needs an account or key, and the app works anywhere in the world.
- The free API is for non-commercial use. Commercial use would need their paid plan or another
  source.
- Some fields are missing far ahead (the chance of precipitation stops after about a week), so
  the model keeps them as `Option` and the views must cope.
- Changing provider means rewriting `forecast.rs`'s parsing and `fetch.rs`. The views only see
  the app's own `Forecast` type.
