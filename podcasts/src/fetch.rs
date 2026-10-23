//! Network requests off the UI thread (Patina's "never block the UI"):
//! feeds, searches and lookups at Apple (ADR-0002), and artwork. Each runs
//! on a short-lived thread and answers through a channel, waking the UI.
//!
//! Also the caches in `~/.cache/podcasts/`: the last copy of every feed, so
//! start-up is instant and works offline, and artwork shrunk to thumbnails.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use crate::download::fnv;
use crate::feed::{self, Found, Show};

const SEARCH_URL: &str = "https://itunes.apple.com/search";
const LOOKUP_URL: &str = "https://itunes.apple.com/lookup";

/// Feeds of a show with thousands of episodes run to tens of megabytes.
const FEED_LIMIT: u64 = 64 * 1024 * 1024;
/// Artwork is kept at this size; feeds offer up to 3000 px.
pub const ART_SIZE: u32 = 256;

pub enum Reply {
    Feed {
        url: String,
        result: Result<Show, String>,
    },
    Search {
        query: String,
        result: Result<Vec<Found>, String>,
    },
    /// The feed behind an Apple Podcasts link.
    Lookup {
        result: Result<String, String>,
    },
    Art {
        url: String,
        result: Result<PathBuf, String>,
    },
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
            .timeout_global(Some(Duration::from_secs(30)))
            .user_agent(concat!("podcasts/", env!("CARGO_PKG_VERSION"), " (Patina)"))
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

    pub fn feed(&self, url: &str) {
        let url = url.to_string();
        self.spawn(move |agent| {
            let result = get(agent, &url, &[], FEED_LIMIT).and_then(|body| feed::parse(&url, &body));
            Reply::Feed { url, result }
        });
    }

    pub fn search(&self, query: &str) {
        let query = query.to_string();
        self.spawn(move |agent| {
            let result = get(
                agent,
                SEARCH_URL,
                &[("media", "podcast"), ("entity", "podcast"), ("limit", "12"), ("term", &query)],
                4 << 20,
            )
            .and_then(|body| feed::parse_found(&String::from_utf8_lossy(&body)));
            Reply::Search { query, result }
        });
    }

    pub fn lookup(&self, apple_id: &str) {
        let id = apple_id.to_string();
        self.spawn(move |agent| {
            let result = get(agent, LOOKUP_URL, &[("id", &id), ("entity", "podcast")], 1 << 20)
                .and_then(|body| feed::parse_found(&String::from_utf8_lossy(&body)))
                .and_then(|found| {
                    found
                        .into_iter()
                        .next()
                        .map(|f| f.feed)
                        .ok_or_else(|| "Apple has no public feed for that podcast".into())
                });
            Reply::Lookup { result }
        });
    }

    /// Fetches artwork into the cache as a thumbnail, unless it's there already.
    pub fn art(&self, url: &str) {
        let url = url.to_string();
        self.spawn(move |agent| {
            let result = art_file(&url).ok_or_else(|| "No cache folder".to_string()).and_then(|path| {
                if path.exists() {
                    return Ok(path);
                }
                let body = get(agent, &url, &[], 32 << 20)?;
                let image = image::load_from_memory(&body).map_err(|e| format!("Artwork: {e}"))?;
                let thumb = image.thumbnail(ART_SIZE, ART_SIZE);
                if let Some(dir) = path.parent() {
                    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
                }
                // Written aside and renamed, so a half-written file is never shown.
                let tmp = path.with_extension("tmp");
                thumb.save_with_format(&tmp, image::ImageFormat::Png).map_err(|e| e.to_string())?;
                std::fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
                Ok(path)
            });
            Reply::Art { url, result }
        });
    }
}

fn get(agent: &ureq::Agent, url: &str, query: &[(&str, &str)], limit: u64) -> Result<Vec<u8>, String> {
    let host = url.split('/').nth(2).unwrap_or(url).to_string();
    let mut req = agent.get(url);
    for (k, v) in query {
        req = req.query(*k, *v);
    }
    let mut resp = req.call().map_err(|e| match e {
        ureq::Error::StatusCode(code) => format!("{host} answered {code}"),
        e => format!("Couldn't reach {host}: {e}"),
    })?;
    resp.body_mut().with_config().limit(limit).read_to_vec().map_err(|e| format!("{host}: {e}"))
}

// ---------------------------------------------------------------------------
// The caches
// ---------------------------------------------------------------------------

fn cache_dir() -> Option<PathBuf> {
    dirs::cache_dir().map(|d| d.join("podcasts"))
}

/// Where a piece of artwork's thumbnail is kept.
pub fn art_file(url: &str) -> Option<PathBuf> {
    cache_dir().map(|d| d.join("art").join(format!("{:016x}.png", fnv(url))))
}

fn shows_file() -> Option<PathBuf> {
    cache_dir().map(|d| d.join("shows.json"))
}

pub fn load_shows() -> HashMap<String, Show> {
    shows_file()
        .and_then(|f| std::fs::read_to_string(f).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save_shows(shows: &HashMap<String, Show>) {
    let Some(file) = shows_file() else { return };
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(text) = serde_json::to_string(shows) {
        let _ = std::fs::write(file, text);
    }
}
