//! Calendars and events as the app holds them, and turning events (with
//! their recurrence rules) into the occurrences a view draws.

use std::collections::HashSet;

use chrono::{DateTime, Duration, NaiveDate, Utc};
use chrono_tz::Tz;

use crate::ics::{self, Component, When};

/// A calendar collection on the server.
#[derive(Debug, Clone, PartialEq)]
pub struct Calendar {
    /// The collection's path, e.g. `/calendars/Personal/`.
    pub href: String,
    pub name: String,
    /// `#RRGGBB` or `#RRGGBBAA`, from the server's `calendar-color`.
    pub color: Option<String>,
    pub ctag: String,
}

/// One `VEVENT`: a single event, a recurring series, or one changed
/// occurrence of a series (`RECURRENCE-ID`).
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    /// The calendar's href.
    pub calendar: String,
    /// The `.ics` file this came from, and its etag.
    pub href: String,
    pub etag: Option<String>,
    /// Which `VEVENT` in that file this is.
    pub index: usize,
    pub uid: String,
    pub summary: String,
    pub location: String,
    pub description: String,
    pub start: When,
    /// Exclusive end.
    pub end: When,
    pub rrule: Option<String>,
    pub exdates: Vec<DateTime<Utc>>,
    pub recurrence_id: Option<DateTime<Utc>>,
}

impl Event {
    pub fn all_day(&self) -> bool {
        matches!(self.start, When::Date(_))
    }

    pub fn recurring(&self) -> bool {
        self.rrule.is_some()
    }
}

/// An event happening at a particular time, ready to draw.
#[derive(Debug, Clone, PartialEq)]
pub struct Occurrence {
    /// Index into the store's events.
    pub event: usize,
    pub start: DateTime<Tz>,
    pub end: DateTime<Tz>,
    pub all_day: bool,
}

impl Occurrence {
    /// The days this occurrence covers, inclusive.
    pub fn days(&self) -> (NaiveDate, NaiveDate) {
        let first = self.start.date_naive();
        // An exclusive end at midnight belongs to the day before.
        let last_instant = if self.end > self.start { self.end - Duration::seconds(1) } else { self.end };
        (first, last_instant.date_naive().max(first))
    }
}

/// Reads every `VEVENT` out of a calendar object.
pub fn events_from_ics(calendar: &str, href: &str, etag: Option<String>, text: &str, local: Tz) -> Vec<Event> {
    let mut out = Vec::new();
    let top = ics::parse(text);
    let mut index = 0;
    for cal in top.iter().filter(|c| c.name == "VCALENDAR") {
        for comp in cal.children.iter().filter(|c| c.name == "VEVENT") {
            if let Some(ev) = event_from(comp, calendar, href, etag.clone(), index, local) {
                out.push(ev);
            }
            index += 1;
        }
    }
    out
}

fn event_from(
    c: &Component,
    calendar: &str,
    href: &str,
    etag: Option<String>,
    index: usize,
    local: Tz,
) -> Option<Event> {
    let start = ics::when(c.get("DTSTART")?, local)?;
    let end = c
        .get("DTEND")
        .and_then(|p| ics::when(p, local))
        .or_else(|| {
            let d = c.get("DURATION").and_then(|p| ics::duration(&p.value))?;
            Some(match start {
                When::Date(day) => When::Date(day + Duration::days(d.num_days().max(1))),
                When::Time(t) => When::Time(t + d),
            })
        })
        .unwrap_or(match start {
            When::Date(day) => When::Date(day + Duration::days(1)),
            When::Time(t) => When::Time(t),
        });
    let text = |name: &str| c.get(name).map(|p| ics::unescape(&p.value)).unwrap_or_default();
    let utc = |w: When| w.instant(local).with_timezone(&Utc);
    let mut exdates = Vec::new();
    for p in c.all("EXDATE") {
        for value in p.value.split(',') {
            let single = ics::Prop { value: value.to_string(), ..p.clone() };
            if let Some(w) = ics::when(&single, local) {
                exdates.push(utc(w));
            }
        }
    }
    Some(Event {
        calendar: calendar.to_string(),
        href: href.to_string(),
        etag,
        index,
        uid: text("UID"),
        summary: text("SUMMARY"),
        location: text("LOCATION"),
        description: text("DESCRIPTION"),
        start,
        end,
        rrule: c.get("RRULE").map(|p| p.value.clone()),
        exdates,
        recurrence_id: c.get("RECURRENCE-ID").and_then(|p| ics::when(p, local)).map(utc),
    })
}

/// The occurrences of `events` that touch the days `from..=to`, for the
/// calendars `visible` lets through, sorted by start.
pub fn occurrences(
    events: &[Event],
    from: NaiveDate,
    to: NaiveDate,
    local: Tz,
    visible: impl Fn(&str) -> bool,
) -> Vec<Occurrence> {
    let window_start = ics::local_midnight(from, local);
    let window_end = ics::local_midnight(to + Duration::days(1), local);
    // Occurrences replaced by a changed copy (same UID, RECURRENCE-ID).
    let overridden: HashSet<(String, DateTime<Utc>)> =
        events.iter().filter_map(|e| e.recurrence_id.map(|r| (e.uid.clone(), r))).collect();

    let mut out = Vec::new();
    for (i, ev) in events.iter().enumerate() {
        if !visible(&ev.calendar) {
            continue;
        }
        let start = ev.start.instant(local);
        let length = ev.end.instant(local) - start;
        let mut push = |s: DateTime<Tz>| {
            let s = s.with_timezone(&local);
            let e = s + length;
            if s < window_end && (e > window_start || (e == s && s >= window_start)) {
                out.push(Occurrence { event: i, start: s, end: e, all_day: ev.all_day() });
            }
        };
        match (&ev.rrule, ev.recurrence_id) {
            (Some(rule), None) => {
                for s in expand(ev, rule, local, window_start - length - Duration::days(1), window_end) {
                    let key = (ev.uid.clone(), s.with_timezone(&Utc));
                    if ev.exdates.contains(&key.1) || overridden.contains(&key) {
                        continue;
                    }
                    push(s);
                }
            }
            _ => push(start),
        }
    }
    out.sort_by_key(|o| (!o.all_day, o.start, o.end));
    out
}

/// The starts of a recurring event between two instants.
fn expand(ev: &Event, rule: &str, local: Tz, after: DateTime<Tz>, before: DateTime<Tz>) -> Vec<DateTime<Tz>> {
    // rrule gets only DTSTART and RRULE; EXDATE and overrides are filtered
    // here, so their many spellings never reach its parser.
    let dtstart = match ev.start {
        When::Date(d) => format!("DTSTART;TZID={}:{}T000000", local.name(), ics::format_date(d)),
        When::Time(t) => format!("DTSTART;TZID={}:{}", t.timezone().name(), t.naive_local().format("%Y%m%dT%H%M%S")),
    };
    let Ok(set) = format!("{dtstart}\nRRULE:{rule}").parse::<rrule::RRuleSet>() else {
        return Vec::new();
    };
    let to_rrule = |t: DateTime<Tz>| t.with_timezone(&rrule::Tz::Tz(Tz::UTC));
    set.after(to_rrule(after))
        .before(to_rrule(before))
        .all(2000)
        .dates
        .into_iter()
        .map(|d| d.with_timezone(&Utc).with_timezone(&local))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stockholm() -> Tz {
        "Europe/Stockholm".parse().unwrap()
    }

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn wrap(body: &str) -> String {
        format!("BEGIN:VCALENDAR\r\nVERSION:2.0\r\n{body}END:VCALENDAR\r\n")
    }

    #[test]
    fn weekly_rule_with_byday_until_and_exdate() {
        let text = wrap(
            "BEGIN:VEVENT\r\nUID:gym\r\nSUMMARY:Gym\r\n\
             DTSTART;TZID=Europe/Stockholm:20261005T070000\r\nDTEND;TZID=Europe/Stockholm:20261005T080000\r\n\
             RRULE:FREQ=WEEKLY;UNTIL=20261031T000000Z;BYDAY=MO,TH\r\n\
             EXDATE;TZID=Europe/Stockholm:20261015T070000\r\nEND:VEVENT\r\n",
        );
        let events = events_from_ics("/calendars/P/", "/calendars/P/gym.ics", None, &text, stockholm());
        let occ = occurrences(&events, day(2026, 10, 12), day(2026, 10, 25), stockholm(), |_| true);
        let days: Vec<String> = occ.iter().map(|o| o.start.format("%a %d %H:%M").to_string()).collect();
        // Thursday the 15th is excluded.
        assert_eq!(days, ["Mon 12 07:00", "Mon 19 07:00", "Thu 22 07:00"]);
    }

    #[test]
    fn a_changed_occurrence_replaces_the_one_in_the_series() {
        let text = wrap(
            "BEGIN:VEVENT\r\nUID:standup\r\nSUMMARY:Standup\r\nDTSTART:20261012T080000Z\r\nDURATION:PT15M\r\nRRULE:FREQ=DAILY;COUNT=3\r\nEND:VEVENT\r\n\
             BEGIN:VEVENT\r\nUID:standup\r\nSUMMARY:Standup (late)\r\nRECURRENCE-ID:20261013T080000Z\r\n\
             DTSTART:20261013T100000Z\r\nDURATION:PT15M\r\nEND:VEVENT\r\n",
        );
        let events = events_from_ics("c", "h", None, &text, stockholm());
        let occ = occurrences(&events, day(2026, 10, 12), day(2026, 10, 14), stockholm(), |_| true);
        let titles: Vec<String> =
            occ.iter().map(|o| format!("{} {}", o.start.format("%d %H:%M"), events[o.event].summary)).collect();
        assert_eq!(titles, ["12 10:00 Standup", "13 12:00 Standup (late)", "14 10:00 Standup"]);
    }

    #[test]
    fn all_day_and_multi_day_events_cover_the_right_days() {
        let text = wrap(
            "BEGIN:VEVENT\r\nUID:trip\r\nSUMMARY:Trip\r\nDTSTART;VALUE=DATE:20261012\r\nDTEND;VALUE=DATE:20261015\r\nEND:VEVENT\r\n",
        );
        let events = events_from_ics("c", "h", None, &text, stockholm());
        let occ = occurrences(&events, day(2026, 10, 14), day(2026, 10, 20), stockholm(), |_| true);
        assert_eq!(occ.len(), 1);
        assert!(occ[0].all_day);
        assert_eq!(occ[0].days(), (day(2026, 10, 12), day(2026, 10, 14)));
        assert!(occurrences(&events, day(2026, 10, 15), day(2026, 10, 20), stockholm(), |_| true).is_empty());
    }

    #[test]
    fn hidden_calendars_are_left_out() {
        let text = wrap("BEGIN:VEVENT\r\nUID:a\r\nDTSTART:20261012T080000Z\r\nEND:VEVENT\r\n");
        let events = events_from_ics("/calendars/Hidden/", "h", None, &text, stockholm());
        assert!(
            occurrences(&events, day(2026, 10, 12), day(2026, 10, 12), stockholm(), |c| c != "/calendars/Hidden/")
                .is_empty()
        );
    }

    #[test]
    fn zero_length_events_still_show() {
        let text = wrap("BEGIN:VEVENT\r\nUID:a\r\nSUMMARY:Ping\r\nDTSTART:20261012T080000Z\r\nEND:VEVENT\r\n");
        let events = events_from_ics("c", "h", None, &text, stockholm());
        assert_eq!(occurrences(&events, day(2026, 10, 12), day(2026, 10, 12), stockholm(), |_| true).len(), 1);
    }
}
