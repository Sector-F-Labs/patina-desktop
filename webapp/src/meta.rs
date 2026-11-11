//! What a site says about itself, read once when a web app is installed or refreshed and
//! stored with it (ADR-0133): its theme colour and its icon, from the page's `<meta
//! name="theme-color">` and `<link rel="icon">` tags and its web app manifest. Nothing here runs
//! at launch, so opening a web app never waits on the network.
//!
//! The page is fetched with `curl`, which every desktop has, rather than a Rust HTTP stack
//! for two small requests.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Deserialize;

/// What a page gave: a colour as `#rrggbb`, and the address of its best icon.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Found {
    pub color: Option<String>,
    pub icon: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct Manifest {
    theme_color: Option<String>,
    background_color: Option<String>,
    #[serde(default)]
    icons: Vec<ManifestIcon>,
}

#[derive(Debug, Deserialize)]
struct ManifestIcon {
    src: String,
    #[serde(default)]
    sizes: String,
}

/// An icon on offer: where, and its largest side in pixels (0 if it didn't say).
type Candidate = (String, u32);

/// Reads the page at `url` and its manifest: with `curl` first, and when the site won't talk
/// to that, with the WebKit host loading the page as its window would (`patina-webview
/// probe`). The colour is the manifest's or the page's, the icon the best of the sizes offered.
pub fn read(url: &str) -> Found {
    // The page as it renders, when there's a display to render on: its colour is what it shows,
    // where a fetched page only says what it declares.
    if let Some(found) = probe(url).filter(|f| *f != Found::default()) {
        return found;
    }
    read_with_curl(url)
}

fn read_with_curl(url: &str) -> Found {
    let Some(html) = get_text(url) else {
        return Found::default();
    };
    let tags = tags(&html);
    let color = tags
        .iter()
        .find(|t| t.name == "meta" && t.attr("name") == Some("theme-color"))
        .and_then(|t| t.attr("content"))
        .map(str::to_string);
    let icons: Vec<(String, String)> = tags
        .iter()
        .filter(|t| t.name == "link")
        .filter(|t| {
            t.attr("rel")
                .is_some_and(|r| r.split_whitespace().any(|w| w.ends_with("icon")))
        })
        .filter_map(|t| Some((resolve(url, t.attr("href")?), t.attr("sizes")?.to_string())))
        .collect();
    let manifest_url = tags
        .iter()
        .find(|t| {
            t.name == "link"
                && t.attr("rel")
                    .is_some_and(|r| r.split_whitespace().any(|w| w == "manifest"))
        })
        .and_then(|t| t.attr("href"))
        .map(|href| resolve(url, href));
    let manifest = manifest_url
        .as_deref()
        .and_then(get_text)
        .and_then(|text| serde_json::from_str::<Manifest>(&text).ok());
    combine(None, color, icons, manifest_url, manifest)
}

/// What the WebKit host's `probe` found in the page once it had run: the page's tags and its
/// manifest, as the page itself sees them.
#[derive(Debug, Default, Deserialize)]
struct Probe {
    /// The colour along the page's top edge as it rendered, in the scheme the machine is in.
    top: Option<String>,
    /// What its `<meta name="theme-color">` declares.
    color: Option<String>,
    #[serde(default)]
    icons: Vec<ProbeIcon>,
    #[serde(rename = "manifestUrl")]
    manifest_url: Option<String>,
    manifest: Option<Manifest>,
}

#[derive(Debug, Deserialize)]
struct ProbeIcon {
    href: String,
    #[serde(default)]
    sizes: String,
}

fn probe(url: &str) -> Option<Found> {
    let webview = crate::find_webview()?;
    let out = Command::new("timeout")
        .arg("45")
        .arg(webview)
        .args(["probe", url])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let probe: Probe = serde_json::from_slice(&out.stdout).ok()?;
    Some(combine(
        probe.top,
        probe.color,
        probe.icons.into_iter().map(|i| (i.href, i.sizes)).collect(),
        probe.manifest_url,
        probe.manifest,
    ))
}

/// The colour and icon from what a page and its manifest said. The colour is the one the page
/// rendered, if it was loaded; else the manifest's theme colour, the page's, then the manifest's
/// background. A declared colour is one for both schemes, so a dark page declares a light one
/// and gets a light frame.
fn combine(
    rendered: Option<String>,
    page_color: Option<String>,
    page_icons: Vec<(String, String)>,
    manifest_url: Option<String>,
    manifest: Option<Manifest>,
) -> Found {
    let mut color = page_color.as_deref().and_then(normalise_color);
    let mut icons: Vec<Candidate> = page_icons
        .into_iter()
        .map(|(href, sizes)| (href, largest_size(&sizes)))
        .collect();
    if let Some(manifest) = manifest {
        color = manifest
            .theme_color
            .as_deref()
            .and_then(normalise_color)
            .or(color)
            .or_else(|| {
                manifest
                    .background_color
                    .as_deref()
                    .and_then(normalise_color)
            });
        let at = manifest_url.unwrap_or_default();
        icons.extend(
            manifest
                .icons
                .iter()
                .map(|i| (resolve(&at, &i.src), largest_size(&i.sizes))),
        );
    }
    Found {
        color: rendered.as_deref().and_then(normalise_color).or(color),
        icon: best_icon(&icons),
    }
}

/// Downloads the icon at `url` into `dir` as `<name>.png`, whatever it was served as (a site's
/// `.png` can be a WebP): the bar and the launcher draw only SVG and PNG icons, so a WebP or JPEG
/// kept as it came would show as a letter. `None` if it isn't a bitmap, such as an SVG or `.ico`.
pub fn download_icon(url: &str, dir: &Path, name: &str) -> Option<PathBuf> {
    let png = to_png(&get(url)?)?;
    std::fs::create_dir_all(dir).ok()?;
    let path = dir.join(format!("{name}.png"));
    std::fs::write(&path, png).ok()?;
    Some(path)
}

/// A PNG, PNG-encoded from a PNG, JPEG or WebP.
fn to_png(bytes: &[u8]) -> Option<Vec<u8>> {
    let image = image::load_from_memory(bytes).ok()?;
    let mut out = std::io::Cursor::new(Vec::new());
    image.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

/// The icon to take: the smallest of those at least 128 pixels, else the largest there is;
/// an SVG or `.ico` is passed over, as the launcher draws only bitmaps.
fn best_icon(icons: &[Candidate]) -> Option<String> {
    let usable: Vec<&Candidate> = icons
        .iter()
        .filter(|(url, _)| {
            let path = url.split(['?', '#']).next().unwrap_or("").to_lowercase();
            !(path.ends_with(".svg") || path.ends_with(".ico"))
        })
        .collect();
    usable
        .iter()
        .filter(|(_, s)| *s >= 128)
        .min_by_key(|(_, s)| *s)
        .or_else(|| usable.iter().max_by_key(|(_, s)| *s))
        .map(|(url, _)| url.clone())
}

/// The largest side in a `sizes` list such as `"48x48 192x192"`; 0 if there's none (`any`).
fn largest_size(sizes: &str) -> u32 {
    sizes
        .split_whitespace()
        .filter_map(|s| s.split_once(['x', 'X']))
        .filter_map(|(w, h)| Some(w.parse::<u32>().ok()?.max(h.parse().ok()?)))
        .max()
        .unwrap_or(0)
}

/// `#rgb`, `#rrggbb` or `#rrggbbaa` as `#rrggbb`; anything else (names, `rgb()`) isn't read.
pub fn normalise_color(css: &str) -> Option<String> {
    let hex = css.trim().strip_prefix('#')?;
    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    match hex.len() {
        3 => Some(format!(
            "#{}",
            hex.chars()
                .flat_map(|c| [c, c])
                .collect::<String>()
                .to_lowercase()
        )),
        6 | 8 => Some(format!("#{}", hex[..6].to_lowercase())),
        _ => None,
    }
}

/// `href` as an address, relative to the page `base` it was found on.
fn resolve(base: &str, href: &str) -> String {
    let href = href.trim();
    if href.contains("://") {
        return href.to_string();
    }
    let (scheme, rest) = base.split_once("://").unwrap_or(("https", base));
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    if let Some(protocol_relative) = href.strip_prefix("//") {
        return format!("{scheme}://{protocol_relative}");
    }
    if href.starts_with('/') {
        return format!("{scheme}://{host}{href}");
    }
    let path = rest[host.len()..].split(['?', '#']).next().unwrap_or("");
    let dir = path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    format!("{scheme}://{host}{dir}/{href}")
}

/// A `<meta>` or `<link>` tag and its attributes (names lower-cased).
#[derive(Debug)]
struct Tag {
    name: &'static str,
    attrs: Vec<(String, String)>,
}

impl Tag {
    fn attr(&self, key: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

/// Every `<meta>` and `<link>` in `html`, which is all it takes to read a head; not a parser.
fn tags(html: &str) -> Vec<Tag> {
    let lower = html.to_ascii_lowercase();
    let mut out = Vec::new();
    for (name, open) in [("meta", "<meta"), ("link", "<link")] {
        let mut from = 0;
        while let Some(i) = lower[from..].find(open) {
            let start = from + i + open.len();
            from = start;
            if !html[start..].starts_with(|c: char| c.is_whitespace()) {
                continue;
            }
            let Some(end) = html[start..].find('>') else {
                break;
            };
            out.push(Tag {
                name,
                attrs: attributes(&html[start..start + end]),
            });
            from = start + end;
        }
    }
    out
}

fn attributes(text: &str) -> Vec<(String, String)> {
    let mut attrs = Vec::new();
    let mut chars = text.char_indices().peekable();
    while let Some(&(i, c)) = chars.peek() {
        if c.is_whitespace() || c == '/' {
            chars.next();
            continue;
        }
        let mut end = text.len();
        while let Some(&(j, c)) = chars.peek() {
            if c == '=' || c.is_whitespace() || c == '/' {
                end = j;
                break;
            }
            chars.next();
        }
        let key = text[i..end].to_ascii_lowercase();
        while chars.peek().is_some_and(|&(_, c)| c.is_whitespace()) {
            chars.next();
        }
        let mut value = String::new();
        if chars.peek().is_some_and(|&(_, c)| c == '=') {
            chars.next();
            while chars.peek().is_some_and(|&(_, c)| c.is_whitespace()) {
                chars.next();
            }
            match chars.peek().copied() {
                Some((_, q @ ('"' | '\''))) => {
                    chars.next();
                    for (_, c) in chars.by_ref() {
                        if c == q {
                            break;
                        }
                        value.push(c);
                    }
                }
                _ => {
                    while let Some(&(_, c)) = chars.peek() {
                        if c.is_whitespace() {
                            break;
                        }
                        value.push(c);
                        chars.next();
                    }
                }
            }
        }
        if !key.is_empty() {
            attrs.push((key, value));
        }
    }
    attrs
}

fn get(url: &str) -> Option<Vec<u8>> {
    let out = Command::new("curl")
        .args([
            "--silent",
            "--location",
            "--fail",
            "--max-time",
            "15",
            "--max-filesize",
            "5000000",
        ])
        .args([
            "--user-agent",
            "patina-webapp (reading a web app's icon and colour)",
        ])
        .arg(url)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status.success().then_some(out.stdout)
}

fn get_text(url: &str) -> Option<String> {
    Some(String::from_utf8_lossy(&get(url)?).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn any_bitmap_becomes_a_png() {
        let pixels = image::RgbImage::from_pixel(4, 4, image::Rgb([200, 30, 30]));
        let mut jpeg = std::io::Cursor::new(Vec::new());
        pixels
            .write_to(&mut jpeg, image::ImageFormat::Jpeg)
            .unwrap();
        let png = to_png(jpeg.get_ref()).unwrap();
        assert_eq!(&png[1..4], b"PNG");
        assert!(to_png(b"<svg xmlns='http://www.w3.org/2000/svg'/>").is_none());
    }

    #[test]
    fn the_rendered_colour_beats_the_declared_one() {
        let manifest = || {
            Some(Manifest {
                theme_color: Some("#ffffff".into()),
                ..Manifest::default()
            })
        };
        // A dark page that declares white: the frame is the page's dark.
        let found = combine(
            Some("#111b21".into()),
            Some("#fff".into()),
            vec![],
            None,
            manifest(),
        );
        assert_eq!(found.color.as_deref(), Some("#111b21"));
        // Nothing rendered (a fetched page): the declaration.
        let found = combine(None, Some("#fff".into()), vec![], None, manifest());
        assert_eq!(found.color.as_deref(), Some("#ffffff"));
        // Something unreadable rendered: the declaration, too.
        let found = combine(Some("transparent".into()), None, vec![], None, manifest());
        assert_eq!(found.color.as_deref(), Some("#ffffff"));
    }

    #[test]
    fn colours_are_normalised() {
        assert_eq!(normalise_color("#0A8"), Some("#00aa88".into()));
        assert_eq!(normalise_color(" #128C7E "), Some("#128c7e".into()));
        assert_eq!(normalise_color("#128c7eff"), Some("#128c7e".into()));
        assert_eq!(normalise_color("red"), None);
        assert_eq!(normalise_color("#12"), None);
    }

    #[test]
    fn addresses_resolve_against_the_page() {
        let base = "https://web.whatsapp.com/a/b?x=1";
        assert_eq!(resolve(base, "https://x.com/i.png"), "https://x.com/i.png");
        assert_eq!(
            resolve(base, "//cdn.x.com/i.png"),
            "https://cdn.x.com/i.png"
        );
        assert_eq!(resolve(base, "/i.png"), "https://web.whatsapp.com/i.png");
        assert_eq!(resolve(base, "i.png"), "https://web.whatsapp.com/a/i.png");
        assert_eq!(resolve("https://x.com", "i.png"), "https://x.com/i.png");
    }

    #[test]
    fn tags_are_read_from_a_head() {
        let html = r#"<html><head><META NAME="theme-color" content='#128C7E'>
            <link rel="icon" sizes="32x32" href="/f.png"/>
            <link rel="manifest" href=/m.json><meta charset=utf-8></head>"#;
        let tags = tags(html);
        let color = tags
            .iter()
            .find(|t| t.attr("name") == Some("theme-color"))
            .unwrap();
        assert_eq!(color.attr("content"), Some("#128C7E"));
        let manifest = tags
            .iter()
            .find(|t| t.attr("rel") == Some("manifest"))
            .unwrap();
        assert_eq!(manifest.attr("href"), Some("/m.json"));
        assert_eq!(tags.len(), 4);
    }

    #[test]
    fn the_icon_is_the_smallest_big_enough_and_a_bitmap() {
        let icons = [
            ("https://x/a.svg".to_string(), 512),
            ("https://x/b.png".to_string(), 48),
            ("https://x/c.png".to_string(), 192),
            ("https://x/d.png".to_string(), 512),
            ("https://x/e.ico".to_string(), 256),
        ];
        assert_eq!(best_icon(&icons), Some("https://x/c.png".into()));
        assert_eq!(best_icon(&icons[..2]), Some("https://x/b.png".into()));
        assert_eq!(best_icon(&[]), None);
        assert_eq!(largest_size("48x48 192x192"), 192);
        assert_eq!(largest_size("any"), 0);
    }
}
