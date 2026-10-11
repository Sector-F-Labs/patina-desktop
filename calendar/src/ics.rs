//! Just enough iCalendar (RFC 5545) for a calendar client: reading the
//! events out of a `.ics` file, expanding recurring ones, and writing an
//! edited or new event back with everything else in the file kept.
//!
//! Times are resolved with chrono-tz by `TZID` name rather than by reading
//! the file's `VTIMEZONE` blocks (ADR-0004).

use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;

/// One content line: `NAME;PARAM=VALUE:value`.
#[derive(Debug, Clone, PartialEq)]
pub struct Prop {
    pub name: String,
    pub params: Vec<(String, String)>,
    pub value: String,
}

impl Prop {
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    fn line(&self) -> String {
        let mut out = self.name.clone();
        for (k, v) in &self.params {
            out.push(';');
            out.push_str(k);
            out.push('=');
            if v.contains([':', ';', ',']) {
                out.push('"');
                out.push_str(v);
                out.push('"');
            } else {
                out.push_str(v);
            }
        }
        out.push(':');
        out.push_str(&self.value);
        out
    }
}

/// A component (`VCALENDAR`, `VEVENT`, `VTIMEZONE`…) with its properties and
/// children, in file order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Component {
    pub name: String,
    pub props: Vec<Prop>,
    pub children: Vec<Component>,
}

impl Component {
    pub fn get(&self, name: &str) -> Option<&Prop> {
        self.props.iter().find(|p| p.name.eq_ignore_ascii_case(name))
    }

    pub fn all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Prop> + 'a {
        self.props.iter().filter(move |p| p.name.eq_ignore_ascii_case(name))
    }

    /// Sets a property, replacing every existing one of that name.
    pub fn set(&mut self, name: &str, params: Vec<(String, String)>, value: String) {
        let at = self.props.iter().position(|p| p.name.eq_ignore_ascii_case(name));
        self.props.retain(|p| !p.name.eq_ignore_ascii_case(name));
        let prop = Prop { name: name.to_string(), params, value };
        match at {
            Some(i) => self.props.insert(i.min(self.props.len()), prop),
            None => self.props.push(prop),
        }
    }

    pub fn remove(&mut self, name: &str) {
        self.props.retain(|p| !p.name.eq_ignore_ascii_case(name));
    }

    fn write(&self, out: &mut String) {
        out.push_str(&format!("BEGIN:{}\r\n", self.name));
        for prop in &self.props {
            fold(&prop.line(), out);
        }
        for child in &self.children {
            child.write(out);
        }
        out.push_str(&format!("END:{}\r\n", self.name));
    }
}

/// Folds a content line at 75 octets, as RFC 5545 asks.
fn fold(line: &str, out: &mut String) {
    let mut width = 0;
    for c in line.chars() {
        if width + c.len_utf8() > 75 {
            out.push_str("\r\n ");
            width = 1;
        }
        out.push(c);
        width += c.len_utf8();
    }
    out.push_str("\r\n");
}

fn parse_line(line: &str) -> Option<Prop> {
    // The value starts at the first colon outside a quoted parameter.
    let mut in_quotes = false;
    let colon = line.char_indices().find_map(|(i, c)| match c {
        '"' => {
            in_quotes = !in_quotes;
            None
        }
        ':' if !in_quotes => Some(i),
        _ => None,
    })?;
    let (head, value) = (&line[..colon], &line[colon + 1..]);
    let mut parts = head.split(';');
    let name = parts.next()?.trim().to_ascii_uppercase();
    let params = parts
        .filter_map(|p| p.split_once('='))
        .map(|(k, v)| (k.to_ascii_uppercase(), v.trim_matches('"').to_string()))
        .collect();
    Some(Prop { name, params, value: value.to_string() })
}

/// Parses a whole file into its top-level components.
pub fn parse(text: &str) -> Vec<Component> {
    // Unfold: a line starting with a space or tab continues the previous one.
    let mut lines: Vec<String> = Vec::new();
    for raw in text.split('\n') {
        let raw = raw.strip_suffix('\r').unwrap_or(raw);
        match raw.chars().next() {
            Some(' ' | '\t') if !lines.is_empty() => lines.last_mut().unwrap().push_str(&raw[1..]),
            _ if raw.is_empty() => {}
            _ => lines.push(raw.to_string()),
        }
    }
    let mut stack: Vec<Component> = Vec::new();
    let mut top = Vec::new();
    for line in lines {
        let Some(prop) = parse_line(&line) else { continue };
        match prop.name.as_str() {
            "BEGIN" => stack.push(Component { name: prop.value.to_ascii_uppercase(), ..Default::default() }),
            "END" => {
                if let Some(done) = stack.pop() {
                    match stack.last_mut() {
                        Some(parent) => parent.children.push(done),
                        None => top.push(done),
                    }
                }
            }
            _ => {
                if let Some(current) = stack.last_mut() {
                    current.props.push(prop);
                }
            }
        }
    }
    top
}

pub fn write(components: &[Component]) -> String {
    let mut out = String::new();
    for c in components {
        c.write(&mut out);
    }
    out
}

/// RFC 5545 TEXT unescaping.
pub fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n' | 'N') => out.push('\n'),
                Some(other) => out.push(other),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

pub fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace(';', "\\;").replace(',', "\\,").replace('\n', "\\n")
}

// ---------------------------------------------------------------------------
// Times
// ---------------------------------------------------------------------------

/// When an event starts or ends.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum When {
    /// An all-day date (`VALUE=DATE`).
    Date(NaiveDate),
    /// An instant, kept in the zone it was written in.
    Time(DateTime<Tz>),
}

impl When {
    /// The instant this starts, with all-day dates at local midnight.
    pub fn instant(&self, local: Tz) -> DateTime<Tz> {
        match self {
            When::Date(d) => local_midnight(*d, local),
            When::Time(t) => *t,
        }
    }
}

pub fn local_midnight(day: NaiveDate, tz: Tz) -> DateTime<Tz> {
    let naive = day.and_time(NaiveTime::MIN);
    tz.from_local_datetime(&naive).earliest().unwrap_or_else(|| tz.from_utc_datetime(&naive))
}

/// Reads a DATE or DATE-TIME property. Floating times (no `Z`, no `TZID`)
/// are taken as `local`.
pub fn when(prop: &Prop, local: Tz) -> Option<When> {
    let v = prop.value.trim();
    if prop.param("VALUE").is_some_and(|x| x.eq_ignore_ascii_case("DATE")) || v.len() == 8 {
        return NaiveDate::parse_from_str(v, "%Y%m%d").ok().map(When::Date);
    }
    if let Some(utc) = v.strip_suffix('Z') {
        let naive = NaiveDateTime::parse_from_str(utc, "%Y%m%dT%H%M%S").ok()?;
        return Some(When::Time(Utc.from_utc_datetime(&naive).with_timezone(&Tz::UTC)));
    }
    let naive = NaiveDateTime::parse_from_str(v, "%Y%m%dT%H%M%S").ok()?;
    let tz = prop.param("TZID").and_then(zone).unwrap_or(local);
    let t = tz.from_local_datetime(&naive).earliest().unwrap_or_else(|| tz.from_utc_datetime(&naive));
    Some(When::Time(t))
}

/// A zone by IANA name, also accepting the `/mozilla.org/…/Europe/Berlin`
/// style some clients write.
pub fn zone(name: &str) -> Option<Tz> {
    let name = name.trim_matches('"');
    name.parse().ok().or_else(|| {
        let parts: Vec<&str> = name.split('/').collect();
        (0..parts.len()).find_map(|i| parts[i..].join("/").parse().ok())
    })
}

/// An RFC 5545 DURATION such as `PT1H30M`, `P1D` or `-PT15M`.
pub fn duration(value: &str) -> Option<Duration> {
    let (sign, rest) = match value.strip_prefix('-') {
        Some(r) => (-1, r),
        None => (1, value.strip_prefix('+').unwrap_or(value)),
    };
    let rest = rest.strip_prefix('P')?;
    let mut total = Duration::zero();
    let mut number = String::new();
    let mut in_time = false;
    for c in rest.chars() {
        match c {
            'T' => in_time = true,
            '0'..='9' => number.push(c),
            unit => {
                let n: i64 = number.parse().ok()?;
                number.clear();
                total += match (unit, in_time) {
                    ('W', _) => Duration::weeks(n),
                    ('D', _) => Duration::days(n),
                    ('H', true) => Duration::hours(n),
                    ('M', true) => Duration::minutes(n),
                    ('S', true) => Duration::seconds(n),
                    _ => return None,
                };
            }
        }
    }
    Some(total * sign)
}

pub fn format_utc(t: DateTime<Tz>) -> String {
    t.with_timezone(&Utc).format("%Y%m%dT%H%M%SZ").to_string()
}

pub fn format_date(d: NaiveDate) -> String {
    d.format("%Y%m%d").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//test//EN\r\nBEGIN:VTIMEZONE\r\nTZID:Europe/Stockholm\r\nEND:VTIMEZONE\r\nBEGIN:VEVENT\r\nUID:abc\r\nSUMMARY:Lunch\\, with Anna\r\nDESCRIPTION:A long description that goes on and on and on so that it has to be\r\n  folded across lines\r\nDTSTART;TZID=Europe/Stockholm:20261012T120000\r\nDTEND;TZID=Europe/Stockholm:20261012T130000\r\nX-CUSTOM;FOO=\"a:b\":kept\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    #[test]
    fn parses_nested_components_unfolds_and_unescapes() {
        let top = parse(SAMPLE);
        assert_eq!(top.len(), 1);
        let cal = &top[0];
        assert_eq!(cal.children.len(), 2);
        let ev = &cal.children[1];
        assert_eq!(unescape(&ev.get("SUMMARY").unwrap().value), "Lunch, with Anna");
        assert!(ev.get("DESCRIPTION").unwrap().value.ends_with("so that it has to be folded across lines"));
        let custom = ev.get("X-CUSTOM").unwrap();
        assert_eq!((custom.param("FOO"), custom.value.as_str()), (Some("a:b"), "kept"));
    }

    #[test]
    fn writing_back_keeps_everything() {
        let top = parse(SAMPLE);
        assert_eq!(parse(&write(&top)), top);
    }

    #[test]
    fn reads_the_three_kinds_of_time() {
        let local: Tz = "Europe/Stockholm".parse().unwrap();
        let p = |line: &str| parse_line(line).unwrap();
        let utc = when(&p("DTSTART:20261012T100000Z"), local).unwrap().instant(local);
        let zoned = when(&p("DTSTART;TZID=Europe/Stockholm:20261012T120000"), local).unwrap().instant(local);
        assert_eq!(utc, zoned, "12:00 in Stockholm in October is 10:00 UTC");
        let floating = when(&p("DTSTART:20261012T120000"), local).unwrap().instant(local);
        assert_eq!(floating, zoned);
        assert_eq!(
            when(&p("DTSTART;VALUE=DATE:20261012"), local),
            Some(When::Date(NaiveDate::from_ymd_opt(2026, 10, 12).unwrap()))
        );
        let moz = when(&p("DTSTART;TZID=/mozilla.org/20050126_1/Europe/Berlin:20261012T120000"), local).unwrap();
        assert_eq!(moz.instant(local), zoned);
    }

    #[test]
    fn durations() {
        assert_eq!(duration("PT1H30M"), Some(Duration::minutes(90)));
        assert_eq!(duration("P1D"), Some(Duration::days(1)));
        assert_eq!(duration("P1W"), Some(Duration::weeks(1)));
        assert_eq!(duration("-PT15M"), Some(Duration::minutes(-15)));
        assert_eq!(duration("junk"), None);
    }

    #[test]
    fn long_lines_fold_at_75_octets() {
        let mut out = String::new();
        fold(&format!("DESCRIPTION:{}", "x".repeat(200)), &mut out);
        assert!(out.split("\r\n").all(|l| l.len() <= 75));
        let top = parse(&format!("BEGIN:VEVENT\r\n{out}END:VEVENT\r\n"));
        assert_eq!(top[0].get("DESCRIPTION").unwrap().value.len(), 200);
    }
}
