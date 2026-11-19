//! Network requests off the UI thread (Patina's "never block the UI"), and
//! the forecast cache in `~/.cache/weather/` that makes start-up instant.
//!
//! Each request runs on a short-lived thread and answers through a channel,
//! waking the UI when it does.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use crate::forecast::{self, Forecast, Place};

const FORECAST_URL: &str = "https://api.open-meteo.com/v1/forecast";
const AIR_URL: &str = "https://air-quality-api.open-meteo.com/v1/air-quality";
const SEARCH_URL: &str = "https://geocoding-api.open-meteo.com/v1/search";

/// Days fetched; the views show 7 or 14 of them.
const FORECAST_DAYS: u32 = 14;

pub enum Reply {
    Forecast { key: String, result: Result<Forecast, String> },
    Search { query: String, result: Result<Vec<Place>, String> },
}

pub struct Fetcher {
    ctx: egui::Context,
    tx: Sender<Reply>,
    rx: Receiver<Reply>,
    agent: ureq::Agent,
}

impl Fetcher {
    pub fn new(ctx: &egui::Context) -> Self {
        let (tx, rx) = channel();
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(20)))
            .user_agent(concat!("weather/", env!("CARGO_PKG_VERSION"), " (Patina)"))
            .build()
            .into();
        Self { ctx: ctx.clone(), tx, rx, agent }
    }

    pub fn poll(&self) -> Vec<Reply> {
        self.rx.try_iter().collect()
    }

    fn spawn(&self, job: impl FnOnce(&ureq::Agent) -> Reply + Send + 'static) {
        let (tx, ctx, agent) = (self.tx.clone(), self.ctx.clone(), self.agent.clone());
        std::thread::spawn(move || {
            let _ = tx.send(job(&agent));
            ctx.request_repaint();
        });
    }

    pub fn forecast(&self, place: &Place) {
        let place = place.clone();
        self.spawn(move |agent| {
            let (lat, lon) = (place.latitude.to_string(), place.longitude.to_string());
            let days = FORECAST_DAYS.to_string();
            let result = get(
                agent,
                FORECAST_URL,
                &[
                    ("latitude", &lat),
                    ("longitude", &lon),
                    ("timezone", "auto"),
                    ("forecast_days", &days),
                    ("current", forecast::CURRENT),
                    ("hourly", forecast::HOURLY),
                    ("daily", forecast::DAILY),
                ],
            )
            .and_then(|body| {
                let air = get(
                    agent,
                    AIR_URL,
                    &[("latitude", &lat), ("longitude", &lon), ("current", "european_aqi,pm2_5,pm10")],
                )
                .ok();
                forecast::parse(&body, air.as_deref())
            });
            Reply::Forecast { key: place.key(), result }
        });
    }

    pub fn search(&self, query: &str) {
        let query = query.to_string();
        self.spawn(move |agent| {
            let result = get(agent, SEARCH_URL, &[("name", &query), ("count", "8"), ("language", "en")])
                .and_then(|body| forecast::parse_search(&body));
            Reply::Search { query, result }
        });
    }
}

fn get(agent: &ureq::Agent, url: &str, query: &[(&str, &str)]) -> Result<String, String> {
    let mut req = agent.get(url);
    for (k, v) in query {
        req = req.query(*k, *v);
    }
    let mut resp = req.call().map_err(|e| match e {
        ureq::Error::StatusCode(code) => format!("Open-Meteo answered {code}"),
        e => format!("Couldn't reach Open-Meteo: {e}"),
    })?;
    resp.body_mut().read_to_string().map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// The cache
// ---------------------------------------------------------------------------

fn cache_file() -> Option<PathBuf> {
    dirs::cache_dir().map(|d| d.join("weather").join("forecasts.json"))
}

pub fn load_cache() -> HashMap<String, Forecast> {
    cache_file()
        .and_then(|f| std::fs::read_to_string(f).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save_cache(forecasts: &HashMap<String, Forecast>) {
    let Some(file) = cache_file() else { return };
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(text) = serde_json::to_string(forecasts) {
        let _ = std::fs::write(file, text);
    }
}
