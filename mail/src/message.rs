//! What the app knows about a message: a `Summary` for the list, from its
//! headers, and the whole `Body` for the reading pane. Both are parsed with
//! mail-parser from the raw bytes IMAP hands back.

use std::collections::HashMap;
use std::fmt;

use mail_parser::{Address, HeaderValue, MessageParser, MimeHeaders, PartType};
use serde::{Deserialize, Serialize};

/// The width HTML-only mail is wrapped to when turned into text.
const HTML_WIDTH: usize = 96;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Addr {
    pub name: Option<String>,
    pub address: String,
}

impl Addr {
    /// The name if there is one, else the address.
    pub fn short(&self) -> &str {
        self.name.as_deref().filter(|n| !n.trim().is_empty()).unwrap_or(&self.address)
    }
}

impl fmt::Display for Addr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.name.as_deref().filter(|n| !n.trim().is_empty()) {
            Some(name) => write!(f, "{name} <{}>", self.address),
            None => f.write_str(&self.address),
        }
    }
}

/// Addresses as a comma-separated line, as typed in the To and Cc fields.
pub fn join(addrs: &[Addr]) -> String {
    addrs.iter().map(Addr::to_string).collect::<Vec<_>>().join(", ")
}

fn addrs(address: Option<&Address<'_>>) -> Vec<Addr> {
    address
        .map(|a| {
            a.iter()
                .filter_map(|a| Some(Addr { name: a.name().map(str::to_string), address: a.address()?.to_string() }))
                .collect()
        })
        .unwrap_or_default()
}

fn ids(value: &HeaderValue<'_>) -> Vec<String> {
    match value {
        HeaderValue::Text(t) => vec![t.to_string()],
        HeaderValue::TextList(l) => l.iter().map(|t| t.to_string()).collect(),
        _ => Vec::new(),
    }
}

/// A row in the message list.
#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    pub uid: u32,
    pub from: Option<Addr>,
    pub subject: String,
    /// Seconds since the epoch; 0 when unknown.
    pub date: i64,
    pub seen: bool,
    pub flagged: bool,
}

impl Summary {
    /// From a message's header block, and the flags IMAP sent with it.
    pub fn parse(uid: u32, header: &[u8], seen: bool, flagged: bool, internal_date: i64) -> Self {
        let parsed = MessageParser::default().parse_headers(header);
        let (from, subject, date) = match &parsed {
            Some(m) => (
                addrs(m.from()).into_iter().next(),
                m.subject().unwrap_or_default().to_string(),
                m.date().map(|d| d.to_timestamp()).unwrap_or(internal_date),
            ),
            None => (None, String::new(), internal_date),
        };
        Summary { uid, from, subject, date, seen, flagged }
    }
}

/// What the message list is sorted by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SortBy {
    #[default]
    Date,
    From,
    Subject,
}

impl SortBy {
    pub const ALL: [SortBy; 3] = [SortBy::Date, SortBy::From, SortBy::Subject];

    pub fn label(self) -> &'static str {
        match self {
            SortBy::Date => "Date",
            SortBy::From => "Sender",
            SortBy::Subject => "Subject",
        }
    }

    /// The two directions, the usual one first: newest first, A to Z.
    pub fn directions(self) -> [&'static str; 2] {
        match self {
            SortBy::Date => ["Newest first", "Oldest first"],
            SortBy::From | SortBy::Subject => ["A to Z", "Z to A"],
        }
    }
}

/// How the message list is ordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Order {
    pub by: SortBy,
    /// The other way from the usual: oldest first, Z to A.
    pub reversed: bool,
    pub unread_first: bool,
    pub starred_first: bool,
}

impl Order {
    /// Sorts `list`. Ties go newest first, whatever the direction.
    pub fn sort(&self, list: &mut [Summary]) {
        let newest = |a: &Summary, b: &Summary| b.date.cmp(&a.date).then(b.uid.cmp(&a.uid));
        list.sort_by(|a, b| {
            let groups = (self.starred_first.then_some(!a.flagged), self.unread_first.then_some(a.seen))
                .cmp(&(self.starred_first.then_some(!b.flagged), self.unread_first.then_some(b.seen)));
            let key = match self.by {
                SortBy::Date => newest(a, b),
                SortBy::From => sender_key(a).cmp(&sender_key(b)),
                SortBy::Subject => subject_key(&a.subject).cmp(&subject_key(&b.subject)),
            };
            let key = if self.reversed { key.reverse() } else { key };
            groups.then(key).then_with(|| newest(a, b))
        });
    }
}

fn sender_key(m: &Summary) -> String {
    m.from.as_ref().map(|a| a.short().trim().to_lowercase()).unwrap_or_default()
}

/// The subject without its "Re:" and "Fwd:" prefixes, so a reply sorts
/// with what it replies to.
fn subject_key(subject: &str) -> String {
    let mut s = subject.trim();
    while let Some(p) = ["re:", "fwd:", "fw:", "aw:", "sv:", "vs:", "wg:", "tr:"]
        .iter()
        .find(|p| s.get(..p.len()).is_some_and(|head| head.eq_ignore_ascii_case(p)))
    {
        s = s[p.len()..].trim_start();
    }
    s.to_lowercase()
}

#[derive(Debug, Clone, PartialEq)]
pub struct Attachment {
    pub name: String,
    pub data: Vec<u8>,
}

/// A whole message, ready to read and to reply to.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Body {
    pub from: Vec<Addr>,
    pub reply_to: Vec<Addr>,
    pub to: Vec<Addr>,
    pub cc: Vec<Addr>,
    pub subject: String,
    pub date: i64,
    pub message_id: Option<String>,
    pub references: Vec<String>,
    /// The text to show: the plain part, or the HTML part turned into text.
    pub text: String,
    /// The HTML part, if there is one, for Blitz to draw (ADR-0005).
    pub html: Option<String>,
    /// Inline parts by Content-ID (without the angle brackets), such as the
    /// images an HTML part shows with `cid:` URLs.
    pub parts: HashMap<String, Vec<u8>>,
    pub attachments: Vec<Attachment>,
}

impl Body {
    pub fn parse(raw: &[u8]) -> Option<Self> {
        let m = MessageParser::default().parse(raw)?;
        let plain = m.text_body.iter().find_map(|&i| match &m.parts.get(i as usize)?.body {
            PartType::Text(t) => Some(t.to_string()),
            _ => None,
        });
        // Only a real HTML part: mail-parser makes HTML up from plain text otherwise.
        let html = m.html_body.iter().find_map(|&i| match &m.parts.get(i as usize)?.body {
            PartType::Html(h) => Some(h.to_string()),
            _ => None,
        });
        let text =
            plain.or_else(|| html2text::from_read(html.as_deref()?.as_bytes(), HTML_WIDTH).ok()).unwrap_or_default();
        let parts = m
            .parts
            .iter()
            .filter_map(|p| {
                let id = p.content_id()?.trim_start_matches('<').trim_end_matches('>');
                Some((id.to_string(), p.contents().to_vec()))
            })
            .collect();
        let attachments = m
            .attachments()
            .enumerate()
            .map(|(i, part)| Attachment {
                name: part.attachment_name().map(str::to_string).unwrap_or_else(|| format!("attachment-{}", i + 1)),
                data: part.contents().to_vec(),
            })
            .collect();
        let mut references = ids(m.references());
        for id in ids(m.in_reply_to()) {
            if !references.contains(&id) {
                references.push(id);
            }
        }
        Some(Body {
            from: addrs(m.from()),
            reply_to: addrs(m.reply_to()),
            to: addrs(m.to()),
            cc: addrs(m.cc()),
            subject: m.subject().unwrap_or_default().to_string(),
            date: m.date().map(|d| d.to_timestamp()).unwrap_or_default(),
            message_id: m.message_id().map(str::to_string),
            references,
            text,
            html,
            parts,
            attachments,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAIN: &[u8] = b"From: Ada Lovelace <ada@example.com>\r\n\
To: me@proton.me, Charles <charles@example.com>\r\n\
Subject: =?UTF-8?Q?Caf=C3=A9_notes?=\r\n\
Date: Mon, 12 Oct 2026 09:30:00 +0200\r\n\
Message-ID: <abc@example.com>\r\n\
References: <root@example.com>\r\n\
In-Reply-To: <parent@example.com>\r\n\
\r\n\
Hello there.\r\n";

    #[test]
    fn summary_from_headers() {
        let s = Summary::parse(7, PLAIN, false, true, 0);
        assert_eq!(s.uid, 7);
        assert_eq!(s.subject, "Café notes");
        assert_eq!(s.from.as_ref().unwrap().short(), "Ada Lovelace");
        assert_eq!(s.date, 1_791_790_200);
        assert!(!s.seen && s.flagged);
    }

    #[test]
    fn plain_body() {
        let b = Body::parse(PLAIN).unwrap();
        assert_eq!(b.text.trim(), "Hello there.");
        assert_eq!(join(&b.to), "me@proton.me, Charles <charles@example.com>");
        assert_eq!(b.message_id.as_deref(), Some("abc@example.com"));
        assert_eq!(b.references, vec!["root@example.com", "parent@example.com"]);
    }

    #[test]
    fn html_only_body_becomes_text() {
        let raw = b"From: a@example.com\r\nSubject: hi\r\nContent-Type: text/html; charset=utf-8\r\n\r\n\
<html><body><p>Hello <b>world</b></p><ul><li>one</li></ul></body></html>\r\n";
        let b = Body::parse(raw).unwrap();
        assert!(b.text.contains("Hello") && b.text.contains("world") && b.text.contains("one"), "{}", b.text);
        assert!(!b.text.contains("<p>"));
        assert!(b.html.unwrap().contains("<b>world</b>"));
    }

    #[test]
    fn plain_mail_has_no_html() {
        assert_eq!(Body::parse(PLAIN).unwrap().html, None);
    }

    #[test]
    fn inline_parts_by_content_id() {
        let raw = b"From: a@example.com\r\nSubject: news\r\nMIME-Version: 1.0\r\n\
Content-Type: multipart/related; boundary=RR\r\n\r\n\
--RR\r\nContent-Type: text/html\r\n\r\n<p>Logo: <img src=\"cid:logo@acme\"></p>\r\n\
--RR\r\nContent-Type: image/png\r\nContent-ID: <logo@acme>\r\nContent-Transfer-Encoding: base64\r\n\r\naGVsbG8=\r\n--RR--\r\n";
        let b = Body::parse(raw).unwrap();
        assert!(b.html.unwrap().contains("cid:logo@acme"));
        assert_eq!(b.parts.get("logo@acme").map(Vec::as_slice), Some(&b"hello"[..]));
    }

    #[test]
    fn attachments_are_named() {
        let raw = b"From: a@example.com\r\nSubject: file\r\nMIME-Version: 1.0\r\n\
Content-Type: multipart/mixed; boundary=XX\r\n\r\n\
--XX\r\nContent-Type: text/plain\r\n\r\nSee attached.\r\n\
--XX\r\nContent-Type: application/octet-stream\r\nContent-Disposition: attachment; filename=\"notes.txt\"\r\n\
Content-Transfer-Encoding: base64\r\n\r\naGVsbG8=\r\n--XX--\r\n";
        let b = Body::parse(raw).unwrap();
        assert_eq!(b.text.trim(), "See attached.");
        assert_eq!(b.attachments, vec![Attachment { name: "notes.txt".into(), data: b"hello".to_vec() }]);
    }

    fn row(uid: u32, from: &str, subject: &str, date: i64, seen: bool, flagged: bool) -> Summary {
        let from = Some(Addr { name: Some(from.into()), address: format!("{uid}@example.com") });
        Summary { uid, from, subject: subject.into(), date, seen, flagged }
    }

    fn uids(order: Order, list: &mut [Summary]) -> Vec<u32> {
        order.sort(list);
        list.iter().map(|m| m.uid).collect()
    }

    #[test]
    fn lists_sort_every_way() {
        let mut list = vec![
            row(1, "carol", "Re: Budget", 300, true, false),
            row(2, "Alice", "Lunch", 100, false, true),
            row(3, "bob", "budget", 200, false, false),
            row(4, "Alice", "Fwd: RE: agenda", 400, true, false),
        ];
        let by = |by| Order { by, ..Default::default() };
        assert_eq!(uids(Order::default(), &mut list), [4, 1, 3, 2]);
        assert_eq!(uids(Order { reversed: true, ..Default::default() }, &mut list), [2, 3, 1, 4]);
        // Same sender: newest first, either way round.
        assert_eq!(uids(by(SortBy::From), &mut list), [4, 2, 3, 1]);
        assert_eq!(uids(Order { reversed: true, ..by(SortBy::From) }, &mut list), [1, 3, 4, 2]);
        // "Re:" and "Fwd:" don't count, nor does case.
        assert_eq!(uids(by(SortBy::Subject), &mut list), [4, 1, 3, 2]);
        assert_eq!(uids(Order { unread_first: true, ..Default::default() }, &mut list), [3, 2, 4, 1]);
        let both = Order { unread_first: true, starred_first: true, ..Default::default() };
        assert_eq!(uids(both, &mut list), [2, 3, 4, 1]);
    }

    #[test]
    fn reply_prefixes_come_off() {
        assert_eq!(subject_key("Re: Fwd:  RE:Plans"), "plans");
        assert_eq!(subject_key("Reading list"), "reading list");
        assert_eq!(subject_key("Ré: x"), "ré: x");
    }
}
