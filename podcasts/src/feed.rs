//! Podcasts as their RSS feeds describe them (ADR-0002): a `Show` with its
//! `Episode`s, parsed from the feed with the `rss` crate and its iTunes
//! extension, and the iTunes Search and Lookup answers used to find feeds.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Show {
    /// The feed's URL, as subscribed to; it identifies the show.
    pub url: String,
    pub title: String,
    pub author: String,
    /// Plain text, from the feed's HTML.
    pub description: String,
    pub link: String,
    pub artwork: Option<String>,
    /// Newest first.
    pub episodes: Vec<Episode>,
    pub fetched: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Episode {
    /// The item's guid, or its audio URL when it has none.
    pub id: String,
    pub title: String,
    pub published: Option<DateTime<Utc>>,
    /// Seconds, from `itunes:duration`.
    pub duration: Option<u64>,
    pub audio: String,
    pub mime: String,
    /// Bytes, as the enclosure claims; often wrong or 0.
    pub length: u64,
    /// Plain text, from the item's HTML show notes.
    pub notes: String,
    pub artwork: Option<String>,
}

impl Episode {
    /// The file extension to save the audio under, which also hints the decoder.
    pub fn extension(&self) -> &'static str {
        let path = self.audio.split(['?', '#']).next().unwrap_or("").to_ascii_lowercase();
        match (self.mime.as_str(), path.rsplit('.').next().unwrap_or("")) {
            (_, "m4a" | "mp4" | "aac") | ("audio/mp4" | "audio/x-m4a" | "audio/aac", _) => "m4a",
            (_, "ogg" | "oga" | "opus") | ("audio/ogg", _) => "ogg",
            (_, "flac") => "flac",
            (_, "wav") => "wav",
            _ => "mp3",
        }
    }
}

pub fn parse(url: &str, xml: &[u8]) -> Result<Show, String> {
    let channel = rss::Channel::read_from(xml).map_err(|e| format!("Not a podcast feed: {e}"))?;
    let itunes = channel.itunes_ext();
    let mut episodes: Vec<Episode> = channel
        .items()
        .iter()
        .filter_map(|item| {
            let enclosure = item.enclosure()?;
            let ext = item.itunes_ext();
            let notes = item
                .content()
                .or(item.description())
                .or(ext.and_then(|e| e.summary()))
                .map(plain_text)
                .unwrap_or_default();
            Some(Episode {
                id: item.guid().map(|g| g.value().to_string()).unwrap_or_else(|| enclosure.url().to_string()),
                title: item.title().unwrap_or("Untitled").trim().to_string(),
                published: item.pub_date().and_then(parse_date),
                duration: ext.and_then(|e| e.duration()).and_then(parse_duration),
                audio: enclosure.url().to_string(),
                mime: enclosure.mime_type().to_string(),
                length: enclosure.length().trim().parse().unwrap_or(0),
                notes,
                artwork: ext.and_then(|e| e.image()).map(str::to_string),
            })
        })
        .collect();
    // Feeds are usually newest first, but not always.
    episodes.sort_by_key(|e| std::cmp::Reverse(e.published));
    Ok(Show {
        url: url.to_string(),
        title: channel.title().trim().to_string(),
        author: itunes.and_then(|i| i.author()).unwrap_or("").trim().to_string(),
        description: plain_text(channel.description()),
        link: channel.link().to_string(),
        artwork: itunes.and_then(|i| i.image()).or(channel.image().map(|i| i.url())).map(str::to_string),
        episodes,
        fetched: Utc::now(),
    })
}

fn parse_date(text: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc2822(text.trim())
        .or_else(|_| DateTime::parse_from_rfc3339(text.trim()))
        .ok()
        .map(|d| d.with_timezone(&Utc))
}

/// `itunes:duration` is seconds, `MM:SS` or `HH:MM:SS`.
pub fn parse_duration(text: &str) -> Option<u64> {
    let mut seconds = 0u64;
    for part in text.trim().split(':') {
        let part = part.split('.').next()?;
        seconds = seconds * 60 + part.parse::<u64>().ok()?;
    }
    Some(seconds)
}

/// Show notes are HTML; the app shows them as text.
pub fn plain_text(html: &str) -> String {
    if !html.contains('<') && !html.contains('&') {
        return html.trim().to_string();
    }
    // Wide, so lines break where the HTML does rather than at a column.
    let text = html2text::config::plain()
        .link_footnotes(false)
        .no_link_wrapping()
        .string_from_read(html.as_bytes(), 10_000)
        .unwrap_or_else(|_| html.to_string());
    // Runs of blank lines become one.
    let mut out = String::new();
    let mut blank = 0;
    for line in text.lines().map(str::trim_end) {
        if line.trim().is_empty() {
            blank += 1;
            continue;
        }
        if !out.is_empty() {
            out.push_str(if blank > 0 { "\n\n" } else { "\n" });
        }
        blank = 0;
        out.push_str(line);
    }
    out
}

// ---------------------------------------------------------------------------
// Finding feeds: Apple's iTunes Search and Lookup APIs (no key)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Found {
    pub title: String,
    pub author: String,
    pub feed: String,
    pub artwork: Option<String>,
}

#[derive(Deserialize)]
struct Answer {
    results: Vec<Entry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    collection_name: Option<String>,
    artist_name: Option<String>,
    feed_url: Option<String>,
    artwork_url100: Option<String>,
}

/// Search and lookup answers, keeping the podcasts that have a public feed.
pub fn parse_found(json: &str) -> Result<Vec<Found>, String> {
    let answer: Answer = serde_json::from_str(json).map_err(|e| format!("Unexpected answer from Apple: {e}"))?;
    Ok(answer
        .results
        .into_iter()
        .filter_map(|e| {
            Some(Found {
                title: e.collection_name.unwrap_or_default(),
                author: e.artist_name.unwrap_or_default(),
                feed: e.feed_url.filter(|f| !f.is_empty())?,
                artwork: e.artwork_url100,
            })
        })
        .collect())
}

/// The podcast id in an Apple Podcasts link, such as
/// `https://podcasts.apple.com/us/podcast/name/id1494880178?i=1000463172684`.
pub fn apple_id(url: &str) -> Option<String> {
    if !url.contains("apple.com") {
        return None;
    }
    let path = url.split(['?', '#']).next()?;
    let id = path.rsplit('/').find_map(|seg| seg.strip_prefix("id"))?;
    (!id.is_empty() && id.bytes().all(|b| b.is_ascii_digit())).then(|| id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FEED: &[u8] = include_bytes!("../testdata/coffee-break-swedish.xml");

    #[test]
    fn parses_a_real_feed() {
        let show = parse("https://example.com/feed", FEED).unwrap();
        assert_eq!(show.title, "Coffee Break Swedish");
        assert!(show.episodes.len() > 50);
        assert!(show.artwork.is_some());
        let first = show.episodes.iter().find(|e| e.title.starts_with("CBSw 1.01 |")).unwrap();
        assert!(first.audio.starts_with("https://"));
        assert!(first.duration.is_some_and(|d| d > 60));
        assert!(first.published.is_some());
        assert!(!first.notes.contains("<p>"));
        assert!(show.episodes.windows(2).all(|w| w[0].published >= w[1].published));
    }

    #[test]
    fn durations() {
        assert_eq!(parse_duration("3723"), Some(3723));
        assert_eq!(parse_duration("02:03"), Some(123));
        assert_eq!(parse_duration("1:02:03"), Some(3723));
        assert_eq!(parse_duration("1:02:03.5"), Some(3723));
        assert_eq!(parse_duration("soon"), None);
    }

    #[test]
    fn apple_links() {
        let url =
            "https://podcasts.apple.com/us/podcast/cbsw-1-01-hur-m%C3%A5r-du-how-are-you/id1494880178?i=1000463172684";
        assert_eq!(apple_id(url).as_deref(), Some("1494880178"));
        assert_eq!(apple_id("https://feeds.acast.com/public/shows/abc"), None);
        assert_eq!(apple_id("https://podcasts.apple.com/us/podcast/x/idea"), None);
    }

    #[test]
    fn notes_as_text() {
        assert_eq!(plain_text("<p>Hej</p><p>Hur mår du?</p>"), "Hej\n\nHur mår du?");
        assert_eq!(plain_text("Just text"), "Just text");
        assert_eq!(plain_text("Fish &amp; chips"), "Fish & chips");
    }

    #[test]
    fn found_keeps_podcasts_with_feeds() {
        let json = r#"{"resultCount":2,"results":[
            {"collectionName":"A","artistName":"Me","feedUrl":"https://a/rss","artworkUrl100":"https://a/100.jpg"},
            {"collectionName":"B","artistName":"Them"}]}"#;
        let found = parse_found(json).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].feed, "https://a/rss");
    }

    #[test]
    fn extensions() {
        let mut e = parse("u", FEED).unwrap().episodes.remove(0);
        assert_eq!(e.extension(), "mp3");
        e.audio = "https://x/y.m4a?token=1".into();
        e.mime = String::new();
        assert_eq!(e.extension(), "m4a");
    }
}
