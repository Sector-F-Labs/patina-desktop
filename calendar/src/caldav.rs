//! A small, blocking CalDAV client: find the calendars, fetch their events,
//! and write events and colours back. Run it off the UI thread (see `sync`).

use std::time::Duration;

use base64::Engine as _;
use ureq::http::{Method, Request};

use crate::model::Calendar;

/// Where the server is and how to log in.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Account {
    /// The server's base URL, e.g. `https://plan.divanv.com/`.
    pub url: String,
    pub username: String,
    pub password: String,
}

impl Account {
    pub fn is_set(&self) -> bool {
        !self.url.trim().is_empty()
    }
}

/// A calendar object as fetched: its href, etag and iCalendar text.
pub struct Object {
    pub href: String,
    pub etag: Option<String>,
    pub ics: String,
}

pub struct Client {
    agent: ureq::Agent,
    account: Account,
    /// `scheme://host[:port]`, for turning server-relative hrefs into URLs.
    origin: String,
}

const DAV: &str = "DAV:";
const CALDAV: &str = "urn:ietf:params:xml:ns:caldav";
const APPLE: &str = "http://apple.com/ns/ical/";
const CALSERVER: &str = "http://calendarserver.org/ns/";

impl Client {
    pub fn new(account: Account) -> Result<Self, String> {
        let url = account.url.trim().trim_end_matches('/').to_string();
        let scheme_end = url.find("://").ok_or("The server address needs to start with https:// or http://")? + 3;
        let host_end = url[scheme_end..].find('/').map_or(url.len(), |i| scheme_end + i);
        let origin = url[..host_end].to_string();
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .allow_non_standard_methods(true)
            .timeout_global(Some(Duration::from_secs(30)))
            .build()
            .new_agent();
        Ok(Self { agent, account, origin })
    }

    fn url(&self, href: &str) -> String {
        if href.starts_with("http://") || href.starts_with("https://") {
            href.to_string()
        } else if href.starts_with('/') {
            format!("{}{href}", self.origin)
        } else {
            format!("{}/{href}", self.account.url.trim_end_matches('/'))
        }
    }

    /// Sends a request and returns the status, headers and body.
    fn send(
        &self,
        method: &str,
        href: &str,
        headers: &[(&str, &str)],
        body: &str,
    ) -> Result<(u16, ureq::http::HeaderMap, String), String> {
        let mut req = Request::builder()
            .method(Method::from_bytes(method.as_bytes()).map_err(|e| e.to_string())?)
            .uri(self.url(href));
        if !self.account.username.is_empty() {
            let token = base64::engine::general_purpose::STANDARD
                .encode(format!("{}:{}", self.account.username, self.account.password));
            req = req.header("Authorization", format!("Basic {token}"));
        }
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        let req = req.body(body.to_string()).map_err(|e| e.to_string())?;
        let mut resp = self.agent.run(req).map_err(|e| format!("Can't reach the server: {e}"))?;
        let status = resp.status().as_u16();
        let headers = resp.headers().clone();
        let text = resp.body_mut().read_to_string().map_err(|e| e.to_string())?;
        match status {
            401 | 403 => Err("The server didn't accept the username and password".into()),
            404 if method == "PROPFIND" => Err(format!("{href} wasn't found on the server")),
            _ => Ok((status, headers, text)),
        }
    }

    fn propfind(&self, href: &str, depth: &str, props: &str) -> Result<Vec<Response>, String> {
        let body = format!(
            r#"<?xml version="1.0" encoding="utf-8"?><D:propfind xmlns:D="DAV:" xmlns:C="{CALDAV}" xmlns:I="{APPLE}" xmlns:CS="{CALSERVER}"><D:prop>{props}</D:prop></D:propfind>"#
        );
        let (status, _, text) = self.send(
            "PROPFIND",
            href,
            &[("Depth", depth), ("Content-Type", "application/xml; charset=utf-8")],
            &body,
        )?;
        if status != 207 {
            return Err(format!("The server answered {status} to PROPFIND {href}"));
        }
        parse_multistatus(&text)
    }

    /// Finds the calendar home: principal, then its `calendar-home-set`,
    /// falling back to the base URL.
    fn calendar_home(&self) -> Result<String, String> {
        let base = self.account.url.clone();
        let principal = self
            .propfind(&base, "0", "<D:current-user-principal/>")
            .ok()
            .and_then(|r| r.into_iter().find_map(|r| r.prop_href("current-user-principal")));
        let Some(principal) = principal else { return Ok(base) };
        let home = self
            .propfind(&principal, "0", "<C:calendar-home-set/>")?
            .into_iter()
            .find_map(|r| r.prop_href("calendar-home-set"));
        Ok(home.unwrap_or(base))
    }

    /// The calendars that hold events (task lists are left out).
    pub fn calendars(&self) -> Result<Vec<Calendar>, String> {
        let home = self.calendar_home()?;
        let responses = self.propfind(
            &home,
            "1",
            "<D:resourcetype/><D:displayname/><I:calendar-color/><CS:getctag/><C:supported-calendar-component-set/>",
        )?;
        let mut out = Vec::new();
        for r in responses {
            if !r.has(CALDAV, "calendar") {
                continue;
            }
            // No component set means any component, events included.
            let comps = r.comps();
            if !comps.is_empty() && !comps.iter().any(|c| c == "VEVENT") {
                continue;
            }
            let name = r
                .text("displayname")
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| r.href.trim_end_matches('/').rsplit('/').next().unwrap_or(&r.href).to_string());
            out.push(Calendar {
                href: r.href.clone(),
                name,
                color: r.text("calendar-color"),
                ctag: r.text("getctag").unwrap_or_default(),
            });
        }
        out.sort_by_key(|c| c.name.to_lowercase());
        Ok(out)
    }

    /// Every event in a calendar.
    pub fn events(&self, calendar: &str) -> Result<Vec<Object>, String> {
        let body = format!(
            r#"<?xml version="1.0" encoding="utf-8"?><C:calendar-query xmlns:D="DAV:" xmlns:C="{CALDAV}"><D:prop><D:getetag/><C:calendar-data/></D:prop><C:filter><C:comp-filter name="VCALENDAR"><C:comp-filter name="VEVENT"/></C:comp-filter></C:filter></C:calendar-query>"#
        );
        let (status, _, text) = self.send(
            "REPORT",
            calendar,
            &[("Depth", "1"), ("Content-Type", "application/xml; charset=utf-8")],
            &body,
        )?;
        if status != 207 {
            return Err(format!("The server answered {status} when asked for {calendar}'s events"));
        }
        Ok(parse_multistatus(&text)?
            .into_iter()
            .filter_map(|r| Some(Object { ics: r.text("calendar-data")?, etag: r.text("getetag"), href: r.href }))
            .collect())
    }

    /// Writes an event. `etag` guards an update against someone else's
    /// change; `None` creates a new file and fails if one exists. Returns
    /// the new etag, when the server gives one.
    pub fn put(&self, href: &str, ics: &str, etag: Option<&str>) -> Result<Option<String>, String> {
        let mut headers = vec![("Content-Type", "text/calendar; charset=utf-8")];
        match etag {
            Some(tag) => headers.push(("If-Match", tag)),
            None => headers.push(("If-None-Match", "*")),
        }
        let (status, resp_headers, _) = self.send("PUT", href, &headers, ics)?;
        match status {
            200..=299 => Ok(resp_headers.get("etag").and_then(|v| v.to_str().ok()).map(str::to_string)),
            412 => Err("The event changed on the server meanwhile; refresh and try again".into()),
            _ => Err(format!("The server answered {status} when saving the event")),
        }
    }

    pub fn delete(&self, href: &str, etag: Option<&str>) -> Result<(), String> {
        let headers: Vec<(&str, &str)> = etag.map(|t| vec![("If-Match", t)]).unwrap_or_default();
        let (status, _, _) = self.send("DELETE", href, &headers, "")?;
        match status {
            200..=299 | 404 => Ok(()),
            412 => Err("The event changed on the server meanwhile; refresh and try again".into()),
            _ => Err(format!("The server answered {status} when deleting the event")),
        }
    }

    /// Sets a calendar's colour on the server, so every client shows it.
    pub fn set_color(&self, calendar: &str, color: &str) -> Result<(), String> {
        let body = format!(
            r#"<?xml version="1.0" encoding="utf-8"?><D:propertyupdate xmlns:D="DAV:" xmlns:I="{APPLE}"><D:set><D:prop><I:calendar-color>{color}</I:calendar-color></D:prop></D:set></D:propertyupdate>"#
        );
        let (status, _, _) =
            self.send("PROPPATCH", calendar, &[("Content-Type", "application/xml; charset=utf-8")], &body)?;
        match status {
            200..=299 => Ok(()),
            _ => Err(format!("The server answered {status} when setting the colour")),
        }
    }
}

/// One `<D:response>` of a multistatus: its href and the properties that
/// came back with status 200.
#[derive(Debug, Default)]
struct Response {
    href: String,
    props: Vec<PropValue>,
}

#[derive(Debug, Default)]
struct PropValue {
    name: String,
    text: String,
    hrefs: Vec<String>,
    /// (namespace, name, `name` attribute) of child elements.
    children: Vec<(String, String, Option<String>)>,
}

impl Response {
    fn find(&self, name: &str) -> Option<&PropValue> {
        self.props.iter().find(|p| p.name == name)
    }

    fn text(&self, name: &str) -> Option<String> {
        self.find(name).map(|p| p.text.clone())
    }

    fn prop_href(&self, name: &str) -> Option<String> {
        self.find(name).and_then(|p| p.hrefs.first().cloned())
    }

    /// Whether `resourcetype` includes `{ns}name`.
    fn has(&self, ns: &str, name: &str) -> bool {
        self.find("resourcetype").is_some_and(|p| p.children.iter().any(|(n, l, _)| n == ns && l == name))
    }

    fn comps(&self) -> Vec<String> {
        self.find("supported-calendar-component-set")
            .map(|p| p.children.iter().filter_map(|(_, _, attr)| attr.clone()).collect())
            .unwrap_or_default()
    }
}

fn parse_multistatus(xml: &str) -> Result<Vec<Response>, String> {
    let doc = roxmltree::Document::parse(xml).map_err(|e| format!("The server sent XML that couldn't be read: {e}"))?;
    let dav = |n: &roxmltree::Node, name: &str| {
        n.is_element() && n.tag_name().name() == name && n.tag_name().namespace() == Some(DAV)
    };
    let mut out = Vec::new();
    for resp in doc.descendants().filter(|n| dav(n, "response")) {
        let href = resp.children().find(|n| dav(n, "href")).and_then(|n| n.text()).unwrap_or_default();
        let mut r = Response { href: percent_decode(href.trim()), props: Vec::new() };
        for propstat in resp.children().filter(|n| dav(n, "propstat")) {
            let ok = propstat
                .children()
                .find(|n| dav(n, "status"))
                .and_then(|n| n.text())
                .is_none_or(|s| s.contains(" 200"));
            if !ok {
                continue;
            }
            let Some(prop) = propstat.children().find(|n| dav(n, "prop")) else { continue };
            for p in prop.children().filter(|n| n.is_element()) {
                let text: String = p.descendants().filter(|n| n.is_text()).filter_map(|n| n.text()).collect();
                r.props.push(PropValue {
                    name: p.tag_name().name().to_string(),
                    text: text.trim().to_string(),
                    hrefs: p
                        .descendants()
                        .filter(|n| dav(n, "href"))
                        .filter_map(|n| n.text())
                        .map(|h| percent_decode(h.trim()))
                        .collect(),
                    children: p
                        .children()
                        .filter(|n| n.is_element())
                        .map(|n| {
                            (
                                n.tag_name().namespace().unwrap_or_default().to_string(),
                                n.tag_name().name().to_string(),
                                n.attribute("name").map(str::to_string),
                            )
                        })
                        .collect(),
                });
            }
        }
        out.push(r);
    }
    Ok(out)
}

/// Decodes `%20` and friends in an href, so it matches what we build.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(b) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            out.push(b);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_multistatus_with_calendars() {
        let xml = r#"<?xml version="1.0"?>
<D:multistatus xmlns:D="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav" xmlns:I="http://apple.com/ns/ical/" xmlns:CS="http://calendarserver.org/ns/">
 <D:response><D:href>/calendars/</D:href><D:propstat><D:prop><D:resourcetype><D:collection/></D:resourcetype></D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat></D:response>
 <D:response><D:href>/calendars/My%20Life/</D:href><D:propstat><D:prop>
   <D:resourcetype><D:collection/><C:calendar/></D:resourcetype><D:displayname>My Life</D:displayname>
   <I:calendar-color>#83D754</I:calendar-color><CS:getctag>42</CS:getctag>
   <C:supported-calendar-component-set><C:comp name="VEVENT"/></C:supported-calendar-component-set>
 </D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat>
 <D:propstat><D:prop><D:owner/></D:prop><D:status>HTTP/1.1 404 Not Found</D:status></D:propstat></D:response>
</D:multistatus>"#;
        let r = parse_multistatus(xml).unwrap();
        assert_eq!(r.len(), 2);
        assert!(!r[0].has(CALDAV, "calendar"));
        assert!(r[1].has(CALDAV, "calendar"));
        assert_eq!(r[1].href, "/calendars/My Life/");
        assert_eq!(r[1].text("calendar-color").as_deref(), Some("#83D754"));
        assert_eq!(r[1].comps(), ["VEVENT"]);
        assert!(r[1].find("owner").is_none(), "404 propstats are skipped");
    }

    #[test]
    fn urls_from_hrefs() {
        let c = Client::new(Account { url: "https://plan.example.com/".into(), ..Default::default() }).unwrap();
        assert_eq!(c.url("/calendars/Personal/"), "https://plan.example.com/calendars/Personal/");
        assert!(Client::new(Account { url: "plan.example.com".into(), ..Default::default() }).is_err());
    }
}

/// Runs against `make dev-server` (a local `plan serve` with seeded data):
/// `CALENDAR_DEV_SERVER=http://127.0.0.1:5299/ cargo test -- --ignored`.
#[cfg(test)]
mod live {
    use super::*;

    fn client() -> Option<Client> {
        let url = std::env::var("CALENDAR_DEV_SERVER").ok()?;
        Some(Client::new(Account { url, username: "test".into(), password: "test".into() }).unwrap())
    }

    #[test]
    #[ignore = "needs make dev-server"]
    fn finds_calendars_reads_and_writes_events() {
        let Some(c) = client() else { return };
        let cals = c.calendars().unwrap();
        let names: Vec<&str> = cals.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"Personal") && names.contains(&"Work"), "{names:?}");
        assert!(!names.contains(&"tasks"), "task lists are left out");
        let work = cals.iter().find(|c| c.name == "Work").unwrap();
        assert_eq!(work.color.as_deref(), Some("#0088FF"));

        let events = c.events(&work.href).unwrap();
        assert!(events.iter().any(|o| o.ics.contains("SUMMARY:Standup")));

        // Create, update with the etag, then delete.
        let href = format!("{}live-test.ics", work.href);
        let ics = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:live-test\r\nSUMMARY:One\r\nDTSTART:20261012T080000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let _ = c.delete(&href, None);
        let etag = c.put(&href, ics, None).unwrap();
        // plan ignores If-None-Match, so new events rely on unique file names.
        c.put(&href, &ics.replace("One", "Two"), etag.as_deref()).unwrap();
        assert!(c.events(&work.href).unwrap().iter().any(|o| o.ics.contains("SUMMARY:Two")));
        c.delete(&href, None).unwrap();
        assert!(!c.events(&work.href).unwrap().iter().any(|o| o.href == href));

        // Colours round-trip through PROPPATCH.
        c.set_color(&work.href, "#123456").unwrap();
        assert_eq!(c.calendars().unwrap().iter().find(|c| c.name == "Work").unwrap().color.as_deref(), Some("#123456"));
        c.set_color(&work.href, "#0088FF").unwrap();
    }
}
