//! The forecast for one place, read from Open-Meteo's JSON (ADR-0002), and
//! what its weather codes mean. Everything is kept in metric units; `units`
//! converts for display.

use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};

/// A saved location.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Place {
    pub name: String,
    /// The county or state and the country, e.g. "Stockholm County, Sweden".
    #[serde(default)]
    pub region: String,
    pub latitude: f64,
    pub longitude: f64,
}

impl Place {
    /// Identifies the place's forecast in the cache and in replies.
    pub fn key(&self) -> String {
        format!("{:.3},{:.3}", self.latitude, self.longitude)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Forecast {
    pub fetched: DateTime<Utc>,
    /// The place's offset from UTC, so "now" can be found in its own time.
    pub utc_offset: i32,
    pub timezone: String,
    pub current: Current,
    pub hours: Vec<Hour>,
    pub days: Vec<Day>,
    pub air: Option<Air>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Current {
    pub time: NaiveDateTime,
    pub temperature: f32,
    pub feels_like: f32,
    pub humidity: f32,
    pub dew_point: f32,
    pub is_day: bool,
    pub code: u8,
    pub wind: f32,
    pub wind_direction: f32,
    pub gusts: f32,
    pub precipitation: f32,
    pub pressure: f32,
    pub cloud_cover: f32,
    /// Metres.
    pub visibility: f32,
    pub uv: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hour {
    pub time: NaiveDateTime,
    pub temperature: f32,
    pub feels_like: f32,
    pub humidity: f32,
    pub dew_point: f32,
    /// Percent; not given far ahead.
    pub precipitation_chance: Option<f32>,
    /// Millimetres in the hour, of which `snow` fell as snow (in water).
    pub precipitation: f32,
    pub snow: f32,
    pub code: u8,
    pub cloud_cover: f32,
    pub visibility: f32,
    pub wind: f32,
    pub wind_direction: f32,
    pub gusts: f32,
    pub uv: f32,
    pub is_day: bool,
    pub pressure: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Day {
    pub date: NaiveDate,
    pub code: u8,
    pub high: f32,
    pub low: f32,
    pub feels_high: f32,
    pub feels_low: f32,
    pub sunrise: Option<NaiveDateTime>,
    pub sunset: Option<NaiveDateTime>,
    /// Seconds of daylight.
    pub daylight: f32,
    pub uv_max: f32,
    pub precipitation: f32,
    pub precipitation_chance: Option<f32>,
    pub precipitation_hours: f32,
    pub wind_max: f32,
    pub gusts_max: f32,
    pub wind_direction: f32,
}

/// Air quality now, from Open-Meteo's air-quality API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Air {
    pub european_aqi: f32,
    pub pm2_5: f32,
    pub pm10: f32,
}

impl Forecast {
    /// The current time where the place is.
    pub fn now(&self) -> NaiveDateTime {
        (Utc::now() + Duration::seconds(self.utc_offset as i64)).naive_utc()
    }

    pub fn today(&self) -> NaiveDate {
        self.now().date()
    }

    /// The hours of one day.
    pub fn hours_of(&self, date: NaiveDate) -> &[Hour] {
        let start = self.hours.iter().position(|h| h.time.date() == date);
        match start {
            Some(s) => {
                let len = self.hours[s..].iter().take_while(|h| h.time.date() == date).count();
                &self.hours[s..s + len]
            }
            None => &[],
        }
    }

    /// The hour now is in, by index.
    pub fn current_hour(&self) -> usize {
        let now = self.now();
        self.hours.iter().rposition(|h| h.time <= now).unwrap_or(0)
    }

    /// The lowest low and highest high over the shown days, for the range bars.
    pub fn span(&self, days: usize) -> (f32, f32) {
        let days = &self.days[..days.min(self.days.len())];
        let low = days.iter().map(|d| d.low).fold(f32::INFINITY, f32::min);
        let high = days.iter().map(|d| d.high).fold(f32::NEG_INFINITY, f32::max);
        (low, high)
    }

    /// The sun's elevation in degrees at a local time here, at the given
    /// place. Below zero is under the horizon.
    pub fn sun_elevation(&self, latitude: f64, longitude: f64, local: NaiveDateTime) -> f64 {
        sun_elevation(latitude, longitude, local - Duration::seconds(self.utc_offset as i64))
    }

    /// Precipitation in the 24 hours from now.
    pub fn next_24h_precipitation(&self) -> f32 {
        let i = self.current_hour();
        self.hours.iter().skip(i).take(24).map(|h| h.precipitation).sum()
    }
}

// ---------------------------------------------------------------------------
// Reading Open-Meteo's JSON
// ---------------------------------------------------------------------------

pub const HOURLY: &str = "temperature_2m,apparent_temperature,relative_humidity_2m,dew_point_2m,\
precipitation_probability,precipitation,snowfall,weather_code,cloud_cover,visibility,wind_speed_10m,\
wind_direction_10m,wind_gusts_10m,uv_index,is_day,pressure_msl";
pub const DAILY: &str = "weather_code,temperature_2m_max,temperature_2m_min,apparent_temperature_max,\
apparent_temperature_min,sunrise,sunset,daylight_duration,uv_index_max,precipitation_sum,\
precipitation_probability_max,precipitation_hours,wind_speed_10m_max,wind_gusts_10m_max,\
wind_direction_10m_dominant";
pub const CURRENT: &str = "temperature_2m,apparent_temperature,relative_humidity_2m,dew_point_2m,is_day,\
weather_code,wind_speed_10m,wind_direction_10m,wind_gusts_10m,precipitation,pressure_msl,cloud_cover,\
visibility,uv_index";

#[derive(Deserialize)]
struct Raw {
    utc_offset_seconds: i32,
    timezone: String,
    current: RawCurrent,
    hourly: RawHourly,
    daily: RawDaily,
}

#[derive(Deserialize)]
struct RawCurrent {
    time: String,
    temperature_2m: Option<f32>,
    apparent_temperature: Option<f32>,
    relative_humidity_2m: Option<f32>,
    dew_point_2m: Option<f32>,
    is_day: Option<u8>,
    weather_code: Option<u8>,
    wind_speed_10m: Option<f32>,
    wind_direction_10m: Option<f32>,
    wind_gusts_10m: Option<f32>,
    precipitation: Option<f32>,
    pressure_msl: Option<f32>,
    cloud_cover: Option<f32>,
    visibility: Option<f32>,
    uv_index: Option<f32>,
}

type Col = Vec<Option<f32>>;

#[derive(Deserialize)]
struct RawHourly {
    time: Vec<String>,
    temperature_2m: Col,
    apparent_temperature: Col,
    relative_humidity_2m: Col,
    dew_point_2m: Col,
    precipitation_probability: Col,
    precipitation: Col,
    /// Centimetres of snow; roughly 0.7 mm of water each.
    snowfall: Col,
    weather_code: Col,
    cloud_cover: Col,
    visibility: Col,
    wind_speed_10m: Col,
    wind_direction_10m: Col,
    wind_gusts_10m: Col,
    uv_index: Col,
    is_day: Col,
    pressure_msl: Col,
}

#[derive(Deserialize)]
struct RawDaily {
    time: Vec<String>,
    weather_code: Col,
    temperature_2m_max: Col,
    temperature_2m_min: Col,
    apparent_temperature_max: Col,
    apparent_temperature_min: Col,
    sunrise: Vec<Option<String>>,
    sunset: Vec<Option<String>>,
    daylight_duration: Col,
    uv_index_max: Col,
    precipitation_sum: Col,
    precipitation_probability_max: Col,
    precipitation_hours: Col,
    wind_speed_10m_max: Col,
    wind_gusts_10m_max: Col,
    wind_direction_10m_dominant: Col,
}

#[derive(Deserialize)]
struct RawAir {
    current: RawAirCurrent,
}

#[derive(Deserialize)]
struct RawAirCurrent {
    european_aqi: Option<f32>,
    pm2_5: Option<f32>,
    pm10: Option<f32>,
}

fn time(s: &str) -> Result<NaiveDateTime, String> {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M").map_err(|e| format!("bad time {s:?}: {e}"))
}

fn at(col: &Col, i: usize) -> f32 {
    col.get(i).copied().flatten().unwrap_or(0.0)
}

/// Reads a forecast response, and optionally an air-quality one.
pub fn parse(forecast: &str, air: Option<&str>) -> Result<Forecast, String> {
    let raw: Raw = serde_json::from_str(forecast).map_err(|e| format!("Unexpected forecast: {e}"))?;
    let c = raw.current;
    let current = Current {
        time: time(&c.time)?,
        temperature: c.temperature_2m.unwrap_or(0.0),
        feels_like: c.apparent_temperature.or(c.temperature_2m).unwrap_or(0.0),
        humidity: c.relative_humidity_2m.unwrap_or(0.0),
        dew_point: c.dew_point_2m.unwrap_or(0.0),
        is_day: c.is_day.unwrap_or(1) == 1,
        code: c.weather_code.unwrap_or(0),
        wind: c.wind_speed_10m.unwrap_or(0.0),
        wind_direction: c.wind_direction_10m.unwrap_or(0.0),
        gusts: c.wind_gusts_10m.unwrap_or(0.0),
        precipitation: c.precipitation.unwrap_or(0.0),
        pressure: c.pressure_msl.unwrap_or(0.0),
        cloud_cover: c.cloud_cover.unwrap_or(0.0),
        visibility: c.visibility.unwrap_or(0.0),
        uv: c.uv_index.unwrap_or(0.0),
    };

    let h = raw.hourly;
    let mut hours = Vec::with_capacity(h.time.len());
    for (i, t) in h.time.iter().enumerate() {
        // Far ahead some models stop; an hour without a temperature is no use.
        if h.temperature_2m.get(i).copied().flatten().is_none() {
            continue;
        }
        let snow_cm = at(&h.snowfall, i);
        hours.push(Hour {
            time: time(t)?,
            temperature: at(&h.temperature_2m, i),
            feels_like: at(&h.apparent_temperature, i),
            humidity: at(&h.relative_humidity_2m, i),
            dew_point: at(&h.dew_point_2m, i),
            precipitation_chance: h.precipitation_probability.get(i).copied().flatten(),
            precipitation: at(&h.precipitation, i),
            snow: (snow_cm * 0.7).min(at(&h.precipitation, i)),
            code: at(&h.weather_code, i) as u8,
            cloud_cover: at(&h.cloud_cover, i),
            visibility: at(&h.visibility, i),
            wind: at(&h.wind_speed_10m, i),
            wind_direction: at(&h.wind_direction_10m, i),
            gusts: at(&h.wind_gusts_10m, i),
            uv: at(&h.uv_index, i),
            is_day: at(&h.is_day, i) >= 1.0,
            pressure: at(&h.pressure_msl, i),
        });
    }

    let d = raw.daily;
    let mut days = Vec::with_capacity(d.time.len());
    for (i, t) in d.time.iter().enumerate() {
        if d.temperature_2m_max.get(i).copied().flatten().is_none() {
            continue;
        }
        let date = NaiveDate::parse_from_str(t, "%Y-%m-%d").map_err(|e| format!("bad date {t:?}: {e}"))?;
        let moment = |v: &Vec<Option<String>>| v.get(i).cloned().flatten().and_then(|s| time(&s).ok());
        days.push(Day {
            date,
            code: at(&d.weather_code, i) as u8,
            high: at(&d.temperature_2m_max, i),
            low: at(&d.temperature_2m_min, i),
            feels_high: at(&d.apparent_temperature_max, i),
            feels_low: at(&d.apparent_temperature_min, i),
            sunrise: moment(&d.sunrise),
            sunset: moment(&d.sunset),
            daylight: at(&d.daylight_duration, i),
            uv_max: at(&d.uv_index_max, i),
            precipitation: at(&d.precipitation_sum, i),
            precipitation_chance: d.precipitation_probability_max.get(i).copied().flatten(),
            precipitation_hours: at(&d.precipitation_hours, i),
            wind_max: at(&d.wind_speed_10m_max, i),
            gusts_max: at(&d.wind_gusts_10m_max, i),
            wind_direction: at(&d.wind_direction_10m_dominant, i),
        });
    }
    if days.is_empty() || hours.is_empty() {
        return Err("The forecast came back empty".into());
    }

    // Air quality is a bonus: a bad or missing answer just leaves its tile out.
    let air = air.and_then(|a| serde_json::from_str::<RawAir>(a).ok()).and_then(|a| {
        Some(Air {
            european_aqi: a.current.european_aqi?,
            pm2_5: a.current.pm2_5?,
            pm10: a.current.pm10.unwrap_or(0.0),
        })
    });

    Ok(Forecast {
        fetched: Utc::now(),
        utc_offset: raw.utc_offset_seconds,
        timezone: raw.timezone,
        current,
        hours,
        days,
        air,
    })
}

#[derive(Deserialize)]
struct RawSearch {
    #[serde(default)]
    results: Vec<RawPlace>,
}

#[derive(Deserialize)]
struct RawPlace {
    name: String,
    latitude: f64,
    longitude: f64,
    admin1: Option<String>,
    country: Option<String>,
}

/// Reads a geocoding search response.
pub fn parse_search(json: &str) -> Result<Vec<Place>, String> {
    let raw: RawSearch = serde_json::from_str(json).map_err(|e| format!("Unexpected search result: {e}"))?;
    Ok(raw
        .results
        .into_iter()
        .map(|p| Place {
            region: [p.admin1, p.country].into_iter().flatten().collect::<Vec<_>>().join(", "),
            name: p.name,
            latitude: p.latitude,
            longitude: p.longitude,
        })
        .collect())
}

// ---------------------------------------------------------------------------
// Weather codes
// ---------------------------------------------------------------------------

/// What a WMO weather code looks like, for icons and colours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sky {
    Clear,
    PartlyCloudy,
    Cloudy,
    Fog,
    Drizzle,
    Rain,
    Sleet,
    Snow,
    Thunder,
}

pub fn sky(code: u8) -> Sky {
    match code {
        0 | 1 => Sky::Clear,
        2 => Sky::PartlyCloudy,
        3 => Sky::Cloudy,
        45 | 48 => Sky::Fog,
        51 | 53 | 55 => Sky::Drizzle,
        56 | 57 | 66 | 67 => Sky::Sleet,
        61 | 63 | 65 | 80..=82 => Sky::Rain,
        71 | 73 | 75 | 77 | 85 | 86 => Sky::Snow,
        95..=99 => Sky::Thunder,
        _ => Sky::Cloudy,
    }
}

/// Showers rather than steady rain or snow: drawn with a sun or moon behind.
pub fn is_shower(code: u8) -> bool {
    matches!(code, 80..=82 | 85 | 86)
}

pub fn describe(code: u8) -> &'static str {
    match code {
        0 => "Clear",
        1 => "Mostly clear",
        2 => "Partly cloudy",
        3 => "Cloudy",
        45 => "Fog",
        48 => "Freezing fog",
        51 => "Light drizzle",
        53 => "Drizzle",
        55 => "Heavy drizzle",
        56 | 57 => "Freezing drizzle",
        61 => "Light rain",
        63 => "Rain",
        65 => "Heavy rain",
        66 | 67 => "Freezing rain",
        71 => "Light snow",
        73 => "Snow",
        75 => "Heavy snow",
        77 => "Snow grains",
        80 => "Light showers",
        81 => "Showers",
        82 => "Heavy showers",
        85 => "Snow showers",
        86 => "Heavy snow showers",
        95 => "Thunderstorms",
        96 | 99 => "Thunderstorms with hail",
        _ => "Unknown",
    }
}

/// The sun's elevation in degrees at a UTC time, by NOAA's approximate
/// solar position equations (within a fraction of a degree).
pub fn sun_elevation(latitude: f64, longitude: f64, utc: NaiveDateTime) -> f64 {
    use chrono::{Datelike, Timelike};
    use std::f64::consts::PI;
    let hour = utc.hour() as f64 + utc.minute() as f64 / 60.0 + utc.second() as f64 / 3600.0;
    let year_days = if utc.date().leap_year() { 366.0 } else { 365.0 };
    let g = 2.0 * PI / year_days * (utc.ordinal() as f64 - 1.0 + (hour - 12.0) / 24.0);
    let eqtime = 229.18
        * (0.000075 + 0.001868 * g.cos()
            - 0.032077 * g.sin()
            - 0.014615 * (2.0 * g).cos()
            - 0.040849 * (2.0 * g).sin());
    let decl = 0.006918 - 0.399912 * g.cos() + 0.070257 * g.sin() - 0.006758 * (2.0 * g).cos()
        + 0.000907 * (2.0 * g).sin()
        - 0.002697 * (3.0 * g).cos()
        + 0.00148 * (3.0 * g).sin();
    // True solar time in minutes, then the hour angle.
    let solar = hour * 60.0 + eqtime + 4.0 * longitude;
    let angle = (solar / 4.0 - 180.0).to_radians();
    let lat = latitude.to_radians();
    let cos_zenith = (lat.sin() * decl.sin() + lat.cos() * decl.cos() * angle.cos()).clamp(-1.0, 1.0);
    90.0 - cos_zenith.acos().to_degrees()
}

/// A compass point for a direction the wind comes from, in degrees.
pub fn compass(degrees: f32) -> &'static str {
    const POINTS: [&str; 16] =
        ["N", "NNE", "NE", "ENE", "E", "ESE", "SE", "SSE", "S", "SSW", "SW", "WSW", "W", "WNW", "NW", "NNW"];
    POINTS[((degrees.rem_euclid(360.0) / 22.5).round() as usize) % 16]
}

pub fn uv_level(uv: f32) -> &'static str {
    match uv {
        u if u < 3.0 => "Low",
        u if u < 6.0 => "Moderate",
        u if u < 8.0 => "High",
        u if u < 11.0 => "Very high",
        _ => "Extreme",
    }
}

/// The European AQI's bands.
pub fn aqi_level(aqi: f32) -> &'static str {
    match aqi {
        a if a <= 20.0 => "Good",
        a if a <= 40.0 => "Fair",
        a if a <= 60.0 => "Moderate",
        a if a <= 80.0 => "Poor",
        a if a <= 100.0 => "Very poor",
        _ => "Extremely poor",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = include_str!("../testdata/stockholm.json");

    #[test]
    fn reads_a_forecast() {
        let f = parse(SAMPLE, None).unwrap();
        assert_eq!(f.timezone, "Europe/Stockholm");
        assert_eq!(f.days.len(), 14);
        assert_eq!(f.hours.len(), 14 * 24);
        let first = f.days[0].date;
        assert_eq!(f.hours_of(first).len(), 24);
        assert!(f.days.iter().all(|d| d.high >= d.low));
        assert!(f.days[0].sunrise.unwrap() < f.days[0].sunset.unwrap());
    }

    #[test]
    fn air_quality_is_optional() {
        let air = r#"{"current":{"european_aqi":16,"pm2_5":2.2,"pm10":5.4}}"#;
        assert_eq!(parse(SAMPLE, Some(air)).unwrap().air.unwrap().european_aqi, 16.0);
        assert!(parse(SAMPLE, Some("not json")).unwrap().air.is_none());
    }

    #[test]
    fn rejects_an_error_page() {
        assert!(parse(r#"{"error":true,"reason":"bad"}"#, None).is_err());
    }

    #[test]
    fn reads_search_results() {
        let json = r#"{"results":[{"name":"Täby","latitude":59.44,"longitude":18.07,"admin1":"Stockholm County","country":"Sweden"}]}"#;
        let places = parse_search(json).unwrap();
        assert_eq!(places[0].region, "Stockholm County, Sweden");
        assert!(parse_search("{}").unwrap().is_empty());
    }

    #[test]
    fn the_sun_at_noon() {
        let at = |s: &str| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M").unwrap();
        // Stockholm at solar noon: about 90 - 59.3 + declination.
        let equinox = sun_elevation(59.33, 18.07, at("2026-03-20T10:48"));
        assert!((equinox - 30.7).abs() < 1.0, "{equinox}");
        let midsummer = sun_elevation(59.33, 18.07, at("2026-06-21T10:48"));
        assert!((midsummer - 54.1).abs() < 1.0, "{midsummer}");
        assert!(sun_elevation(59.33, 18.07, at("2026-06-21T22:48")) < 0.0);
    }

    #[test]
    fn compass_points() {
        assert_eq!(compass(0.0), "N");
        assert_eq!(compass(359.0), "N");
        assert_eq!(compass(225.0), "SW");
    }
}
