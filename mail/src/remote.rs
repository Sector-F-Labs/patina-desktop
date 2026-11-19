//! Remote images, by choice (ADR-0006). `prepare` lists a message's remote
//! `<img>`s from what its HTML declares, before Blitz sees it: suspected
//! tracking pixels (declared tiny, or hidden) become a box explaining them,
//! the rest become placeholders, unless the user approved them. `Fetcher`
//! downloads approved images off the UI thread.

use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use lol_html::html_content::ContentType;
use lol_html::{RewriteStrSettings, element, rewrite_str};

/// What the tracking-pixel box links to.
pub const EXPLAINER: &str = "https://www.eff.org/deeplinks/2019/01/stop-tracking-my-emails";

/// Images declaring a side this small or smaller are suspected tracking pixels.
const TINY: f32 = 4.0;

/// Approved images bigger than this aren't loaded.
const MAX_BYTES: u64 = 10 * 1024 * 1024;

/// The attribute `prepare` marks each remote image with, holding its index.
pub const MARK: &str = "data-mail-img";

/// A remote image in a message.
#[derive(Debug, Clone, PartialEq)]
pub struct RemoteImage {
    /// Normalised, as Blitz will ask for it.
    pub url: String,
    pub host: String,
    pub width: Option<f32>,
    pub height: Option<f32>,
    pub hidden: bool,
    /// The address looks like a tracker's: a long unique-looking ID, or
    /// words such as open, track, pixel, beacon.
    pub trackerish: bool,
}

impl RemoteImage {
    /// Declares a side of 4 px or less, or hides itself.
    pub fn suspected_pixel(&self) -> bool {
        self.hidden || self.width.is_some_and(|w| w <= TINY) || self.height.is_some_and(|h| h <= TINY)
    }

    pub fn flagged(&self) -> bool {
        self.suspected_pixel() || self.trackerish
    }

    /// "600×200", "600 wide", or "no size given".
    pub fn size_text(&self) -> String {
        match (self.width, self.height) {
            (Some(w), Some(h)) => format!("{w}×{h}"),
            (Some(w), None) => format!("{w} wide"),
            (None, Some(h)) => format!("{h} high"),
            (None, None) => "no size given".into(),
        }
    }
}

/// Rewrites `html` for Blitz and lists its remote images. Images in
/// `approved` are left to load (the guard serves them once fetched); other
/// suspected pixels become the explainer box, and other images are marked
/// as blocked placeholders.
pub fn prepare(html: &str, approved: &HashSet<String>) -> (String, Vec<RemoteImage>) {
    let images = RefCell::new(Vec::new());
    let rewritten = rewrite_str(
        html,
        RewriteStrSettings {
            element_content_handlers: vec![element!("img", |el| {
                // Candidates in srcset would bypass the list; src is enough.
                el.remove_attribute("srcset");
                let Some(url) = el.get_attribute("src").and_then(|s| remote_url(&s)) else { return Ok(()) };
                let style = el.get_attribute("style").unwrap_or_default().to_ascii_lowercase();
                let declared = |attr: &str| {
                    style_px(&style, attr).or_else(|| el.get_attribute(attr).and_then(|v| leading_px(&v)))
                };
                let image = RemoteImage {
                    host: url::Url::parse(&url).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_default(),
                    width: declared("width"),
                    height: declared("height"),
                    hidden: el.has_attribute("hidden") || hides(&style),
                    trackerish: looks_like_tracker(&url),
                    url: url.clone(),
                };
                let index = images.borrow().len().to_string();
                if approved.contains(&url) {
                    el.set_attribute("src", &url)?;
                    el.set_attribute(MARK, &index)?;
                } else if image.suspected_pixel() {
                    el.replace(
                        &format!(
                            r#"<a class="mail-tracker" {MARK}="{index}" href="{EXPLAINER}">Suspected tracking pixel</a>"#
                        ),
                        ContentType::Html,
                    );
                } else {
                    el.set_attribute("src", &url)?;
                    el.set_attribute(MARK, &index)?;
                    el.set_attribute("data-mail-blocked", "")?;
                }
                images.borrow_mut().push(image);
                Ok(())
            })],
            ..RewriteStrSettings::new()
        },
    )
    // lol_html only fails on its memory limit; fall back to the message as it was.
    .unwrap_or_else(|_| html.to_string());
    (rewritten, images.into_inner())
}

/// An http, https or `//` address, normalised; anything else is `None`.
fn remote_url(src: &str) -> Option<String> {
    let src = src.trim();
    let src = if src.starts_with("//") { format!("https:{src}") } else { src.to_string() };
    let url = url::Url::parse(&src).ok()?;
    matches!(url.scheme(), "http" | "https").then(|| url.to_string())
}

/// `width: 1px` and the like in an inline style. Percentages and other
/// units aren't sizes we can judge, so they're `None`.
fn style_px(style: &str, property: &str) -> Option<f32> {
    style.split(';').find_map(|decl| {
        let (name, value) = decl.split_once(':')?;
        (name.trim() == property).then(|| leading_px(value.trim().trim_end_matches("!important").trim()))?
    })
}

fn leading_px(value: &str) -> Option<f32> {
    let value = value.trim();
    let digits: String = value.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    let rest = value[digits.len()..].trim();
    (rest.is_empty() || rest == "px").then(|| digits.parse().ok())?
}

fn hides(style: &str) -> bool {
    let squashed: String = style.chars().filter(|c| !c.is_whitespace()).collect();
    ["display:none", "visibility:hidden", "opacity:0;", "max-height:0", "max-width:0"]
        .iter()
        .any(|h| squashed.contains(h) || squashed.ends_with(h.trim_end_matches(';')))
}

/// Words trackers put in their addresses, and IDs long enough to be unique
/// per recipient.
pub fn looks_like_tracker(url: &str) -> bool {
    const WORDS: &[&str] = &["open", "opened", "track", "tracking", "tracker", "pixel", "beacon", "trk", "wf"];
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let path = rest.split_once('/').map_or("", |(_, p)| p).to_ascii_lowercase();
    path.split(|c: char| !c.is_ascii_alphanumeric()).any(|token| {
        WORDS.contains(&token)
            || (token.len() >= 20
                && token.chars().any(|c| c.is_ascii_digit())
                && token.chars().any(|c| c.is_ascii_alphabetic()))
            || (token.len() >= 24 && token.chars().all(|c| c.is_ascii_hexdigit()))
    })
}

/// Downloads approved images on a thread of their own, and hands back each
/// result with its URL.
pub struct Fetcher {
    tx: Sender<(String, Result<Vec<u8>, String>)>,
    rx: Receiver<(String, Result<Vec<u8>, String>)>,
    ctx: egui::Context,
}

impl Fetcher {
    pub fn new(ctx: &egui::Context) -> Self {
        let (tx, rx) = mpsc::channel();
        Self { tx, rx, ctx: ctx.clone() }
    }

    pub fn fetch(&self, urls: Vec<String>) {
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::Builder::new()
            .name("images".into())
            .spawn(move || {
                // No cookie store (ureq has none without its cookies feature), no
                // referrer, and a plain user agent, so a request says as little as it can.
                let agent: ureq::Agent = ureq::Agent::config_builder()
                    .timeout_global(Some(Duration::from_secs(15)))
                    .max_redirects(5)
                    .user_agent("Mozilla/5.0")
                    .build()
                    .into();
                for url in urls {
                    let result = download(&agent, &url);
                    let _ = tx.send((url, result));
                    ctx.request_repaint();
                }
            })
            .expect("start the image thread");
    }

    pub fn poll(&self) -> Vec<(String, Result<Vec<u8>, String>)> {
        self.rx.try_iter().collect()
    }
}

fn download(agent: &ureq::Agent, url: &str) -> Result<Vec<u8>, String> {
    let mut response = agent.get(url).call().map_err(|e| e.to_string())?;
    let kind = response.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or_default();
    if !kind.trim().to_ascii_lowercase().starts_with("image/") {
        return Err(format!("not an image ({kind})"));
    }
    response.body_mut().with_config().limit(MAX_BYTES).read_to_vec().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAIL: &str = r#"<p>Hi</p>
        <img src="https://cdn.example/hero.jpg" width="600" height="200" srcset="https://cdn.example/hero@2x.jpg 2x">
        <img src="//cdn.example/logo.png" style="width: 120px">
        <img src="https://t.example/open.gif?u=1" width="1" height="1">
        <img src="https://t.example/x.gif" style="display:none">
        <img src="cid:logo@acme">
        <img src="data:image/png;base64,AAAA">"#;

    #[test]
    fn lists_remote_images_with_what_they_declare() {
        let (_, images) = prepare(MAIL, &HashSet::new());
        let urls: Vec<&str> = images.iter().map(|i| i.url.as_str()).collect();
        assert_eq!(
            urls,
            vec![
                "https://cdn.example/hero.jpg",
                "https://cdn.example/logo.png",
                "https://t.example/open.gif?u=1",
                "https://t.example/x.gif"
            ]
        );
        assert_eq!((images[0].width, images[0].height), (Some(600.0), Some(200.0)));
        assert_eq!((images[1].width, images[1].height), (Some(120.0), None));
        assert!(!images[0].flagged() && !images[1].flagged());
        assert!(images[2].suspected_pixel() && images[2].trackerish);
        assert!(images[3].hidden && images[3].suspected_pixel());
        assert_eq!(images[1].host, "cdn.example");
    }

    #[test]
    fn pixels_become_the_explainer_and_the_rest_placeholders() {
        let (html, _) = prepare(MAIL, &HashSet::new());
        assert_eq!(html.matches("Suspected tracking pixel").count(), 2);
        assert!(html.contains(EXPLAINER));
        assert!(!html.contains("open.gif"));
        assert!(html.contains(r#"src="https://cdn.example/logo.png""#), "{html}");
        assert_eq!(html.matches("data-mail-blocked").count(), 2);
        assert!(!html.contains("srcset"));
        assert!(html.contains("cid:logo@acme") && html.contains("data:image/png"));
    }

    #[test]
    fn approved_images_load_even_pixels() {
        let approved =
            HashSet::from(["https://cdn.example/hero.jpg".to_string(), "https://t.example/open.gif?u=1".to_string()]);
        let (html, images) = prepare(MAIL, &approved);
        assert_eq!(images.len(), 4);
        assert!(html.contains(r#"src="https://t.example/open.gif?u=1""#));
        assert_eq!(html.matches("Suspected tracking pixel").count(), 1);
        assert_eq!(html.matches("data-mail-blocked").count(), 1);
    }

    #[test]
    fn sizes_from_attributes_and_styles() {
        assert_eq!(leading_px("1"), Some(1.0));
        assert_eq!(leading_px("2px"), Some(2.0));
        assert_eq!(leading_px("100%"), None);
        assert_eq!(style_px("border:0; width:1px !important;height:auto", "width"), Some(1.0));
        assert_eq!(style_px("max-width:1px", "width"), None);
        assert!(hides("opacity: 0") && hides("visibility : hidden;") && !hides("opacity:0.5"));
    }

    /// A one-request HTTP server on localhost answering with `content_type`.
    fn serve_once(content_type: &'static str, body: &'static [u8]) -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 2048];
            let n = stream.read(&mut request).unwrap();
            let request = String::from_utf8_lossy(&request[..n]).to_ascii_lowercase();
            // What a fetched image tells its server: no cookies, no referrer.
            assert!(!request.contains("cookie:") && !request.contains("referer:"), "{request}");
            let head =
                format!("HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\r\n", body.len());
            stream.write_all(head.as_bytes()).unwrap();
            stream.write_all(body).unwrap();
        });
        format!("http://{addr}/hero.png")
    }

    #[test]
    fn downloads_images_and_refuses_anything_else() {
        let agent: ureq::Agent = ureq::Agent::config_builder().build().into();
        assert_eq!(download(&agent, &serve_once("image/png", b"PNGDATA")).unwrap(), b"PNGDATA");
        let err = download(&agent, &serve_once("text/html", b"<script>")).unwrap_err();
        assert!(err.contains("not an image"), "{err}");
    }

    #[test]
    fn tracker_looking_addresses() {
        assert!(looks_like_tracker("https://list.example/track/open.php?u=3"));
        assert!(looks_like_tracker("https://u123.ct.sendgrid.net/wf/open?upn=abc"));
        assert!(looks_like_tracker("https://cdn.example/i/8f3a9c2e7b1d4f6a0e5c9b2d7a1f3e8c.gif"));
        assert!(looks_like_tracker("https://mail.example/e/a1b2c3d4e5f6g7h8i9j0k1l2"));
        assert!(!looks_like_tracker("https://cdn.example/images/hero-autumn.jpg"));
        assert!(!looks_like_tracker("https://opensource.example/logo.png"));
        assert!(!looks_like_tracker("https://cdn.example/opengraph/banner.png"));
    }
}
