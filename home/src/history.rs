//! Past values of numeric sensors, for the graphs (ADR-0006): what Home Assistant's history API
//! returns, thinned for drawing, and the cache of what's been fetched.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use serde_json::Value;

/// A graph's points are `[seconds since the epoch, value]`.
pub type Points = Vec<[f64; 2]>;

/// How long fetched history is used before it's fetched again.
const STALE: Duration = Duration::from_secs(60);

/// The most points a graph gets; more are averaged into buckets.
const MAX_POINTS: usize = 400;

/// The hours graphs can show.
pub const RANGES: [(u32, &str); 3] = [(6, "6 hours"), (24, "24 hours"), (168, "7 days")];

pub struct Cached {
    pub points: Points,
    fetched: Instant,
}

/// History by (entity id, hours), and what's being fetched.
#[derive(Default)]
pub struct Store {
    cache: HashMap<(String, u32), Cached>,
    pending: HashSet<(String, u32)>,
}

impl Store {
    pub fn get(&self, id: &str, hours: u32) -> Option<&Points> {
        self.cache.get(&(id.to_string(), hours)).map(|c| &c.points)
    }

    /// Whether `id` over `hours` should be fetched now: it's missing or stale, and not on its way.
    pub fn wants(&self, id: &str, hours: u32) -> bool {
        let key = (id.to_string(), hours);
        !self.pending.contains(&key) && self.cache.get(&key).is_none_or(|c| c.fetched.elapsed() > STALE)
    }

    pub fn mark_pending(&mut self, id: &str, hours: u32) {
        self.pending.insert((id.to_string(), hours));
    }

    pub fn insert(&mut self, id: &str, hours: u32, points: Points) {
        let key = (id.to_string(), hours);
        self.pending.remove(&key);
        self.cache.insert(key, Cached { points: downsample(points, MAX_POINTS), fetched: Instant::now() });
    }

    /// Forgets what was on its way, after a failure, so it's asked for again.
    pub fn clear_pending(&mut self) {
        self.pending.clear();
    }
}

/// `/api/history/period` with `minimal_response`: a list per entity, the first item naming it.
/// States that aren't numbers (`unavailable`) are left out.
pub fn parse(body: &Value) -> HashMap<String, Points> {
    let mut series = HashMap::new();
    for list in body.as_array().into_iter().flatten() {
        let Some(list) = list.as_array() else { continue };
        let Some(id) = list.first().and_then(|s| s["entity_id"].as_str()) else { continue };
        let points = list
            .iter()
            .filter_map(|s| {
                let value: f64 = s["state"].as_str()?.parse().ok()?;
                let time = s["last_changed"].as_str().or(s["last_updated"].as_str())?;
                let seconds = chrono::DateTime::parse_from_rfc3339(time).ok()?.timestamp_millis() as f64 / 1000.0;
                value.is_finite().then_some([seconds, value])
            })
            .collect();
        series.insert(id.to_string(), points);
    }
    series
}

/// Averages points into time buckets when there are more than `max`.
pub fn downsample(points: Points, max: usize) -> Points {
    if points.len() <= max || max < 2 {
        return points;
    }
    let (start, end) = (points[0][0], points[points.len() - 1][0]);
    let width = (end - start) / max as f64;
    if width <= 0.0 {
        return points;
    }
    let mut buckets: Vec<(f64, f64, usize)> = vec![(0.0, 0.0, 0); max];
    for [t, v] in &points {
        let b = &mut buckets[(((t - start) / width) as usize).min(max - 1)];
        *b = (b.0 + t, b.1 + v, b.2 + 1);
    }
    buckets.into_iter().filter(|b| b.2 > 0).map(|(t, v, n)| [t / n as f64, v / n as f64]).collect()
}

/// A made-up day or week for the demo entities, ending at the entity's current value.
pub fn demo(id: &str, hours: u32, now: f64, current: f64) -> Points {
    let steps = (hours * 6).min(600) as usize;
    let seed = id.bytes().fold(0.0, |a, b| a + b as f64);
    (0..=steps)
        .map(|i| {
            let f = i as f64 / steps as f64;
            let t = now - (1.0 - f) * hours as f64 * 3600.0;
            let day = (f * hours as f64 / 24.0 * std::f64::consts::TAU).sin();
            let wobble = (f * 40.0 + seed).sin() * 0.5 + (f * 9.0 + seed).cos() * 0.5;
            let value = if id.contains("power") {
                (current * (0.55 + 0.25 * day.abs() + 0.2 * wobble) * if wobble > 0.85 { 2.2 } else { 1.0 }).max(0.0)
            } else {
                current + 3.0 * day + 0.6 * wobble
            };
            [t, if i == steps { current } else { value }]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_a_minimal_response() {
        let body = json!([
            [
                { "entity_id": "sensor.t", "state": "20.5", "last_changed": "2026-10-28T10:00:00+00:00" },
                { "state": "unavailable", "last_changed": "2026-10-28T10:05:00+00:00" },
                { "state": "21", "last_changed": "2026-10-28T10:10:00.500000+00:00" }
            ],
            []
        ]);
        let series = parse(&body);
        assert_eq!(series.len(), 1);
        let t = &series["sensor.t"];
        assert_eq!(t.len(), 2);
        assert_eq!(t[0][1], 20.5);
        assert_eq!(t[1][0] - t[0][0], 600.5);
    }

    #[test]
    fn thins_long_series_by_averaging() {
        let points: Points = (0..1000).map(|i| [i as f64, (i % 2) as f64]).collect();
        let thin = downsample(points.clone(), 100);
        assert!(thin.len() <= 100);
        assert!(thin.windows(2).all(|w| w[0][0] < w[1][0]));
        assert_eq!(downsample(points[..50].to_vec(), 100).len(), 50);
    }

    #[test]
    fn the_store_asks_once_until_it_is_stale() {
        let mut store = Store::default();
        assert!(store.wants("sensor.t", 24));
        store.mark_pending("sensor.t", 24);
        assert!(!store.wants("sensor.t", 24));
        store.insert("sensor.t", 24, vec![[0.0, 1.0]]);
        assert!(!store.wants("sensor.t", 24));
        assert!(store.wants("sensor.t", 6));
        assert_eq!(store.get("sensor.t", 24).map(Vec::len), Some(1));
    }
}
