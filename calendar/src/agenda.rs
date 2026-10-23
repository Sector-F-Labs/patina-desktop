//! `calendar --agenda`: the coming days' events as JSON, for other programs
//! (the Patina desktop's clock). Read from the disk cache the app keeps
//! (ADR-0003), with no window and no network, so it answers whether the app
//! is running or not; it's as fresh as the app's last sync.

use std::collections::HashMap;

use chrono::{Duration, NaiveDate};
use chrono_tz::Tz;
use serde::Serialize;

use crate::model;
use crate::sync::Snapshot;

/// One occurrence, as printed.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Item {
    pub uid: String,
    pub title: String,
    pub location: String,
    /// The calendar's name, and its colour (`#RRGGBB` or `#RRGGBBAA`) where
    /// it has one.
    pub calendar: String,
    pub color: Option<String>,
    /// RFC 3339, in the local zone; an all-day event's start is its first
    /// day's midnight, its end the midnight after its last.
    pub start: String,
    pub end: String,
    pub all_day: bool,
}

/// The occurrences on `days` days from `from`, in calendars not `hidden`:
/// by day, all-day ones first, then by start.
pub fn agenda(snapshot: &Snapshot, hidden: &[String], local: Tz, from: NaiveDate, days: u32) -> Vec<Item> {
    let events: Vec<model::Event> = snapshot
        .objects
        .iter()
        .flat_map(|o| model::events_from_ics(&o.calendar, &o.href, o.etag.clone(), &o.ics, local))
        .collect();
    let calendars: HashMap<&str, _> = snapshot.calendars.iter().map(|c| (c.href.as_str(), c)).collect();
    let to = from + Duration::days(i64::from(days.max(1)) - 1);
    let mut occurrences = model::occurrences(&events, from, to, local, |c| !hidden.iter().any(|h| h == c));
    // By day first: `occurrences` puts every all-day event ahead of the
    // timed ones, whatever their day. An event begun before `from` counts
    // from `from`.
    occurrences.sort_by_key(|o| (o.days().0.max(from), !o.all_day, o.start, o.end));
    occurrences
        .iter()
        .map(|o| {
            let event = &events[o.event];
            let calendar = calendars.get(event.calendar.as_str());
            Item {
                uid: event.uid.clone(),
                title: event.summary.clone(),
                location: event.location.clone(),
                calendar: calendar.map_or_else(|| event.calendar.clone(), |c| c.name.clone()),
                color: calendar.and_then(|c| c.color.clone()),
                start: o.start.to_rfc3339(),
                end: o.end.to_rfc3339(),
                all_day: o.all_day,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::{CachedCalendar, Stored};

    fn stockholm() -> Tz {
        "Europe/Stockholm".parse().unwrap()
    }

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn object(calendar: &str, body: &str) -> Stored {
        Stored {
            calendar: calendar.into(),
            href: format!("{calendar}x.ics"),
            etag: None,
            ics: format!("BEGIN:VCALENDAR\r\nVERSION:2.0\r\n{body}END:VCALENDAR\r\n"),
        }
    }

    fn snapshot() -> Snapshot {
        Snapshot {
            calendars: vec![
                CachedCalendar {
                    href: "/calendars/P/".into(),
                    name: "Personal".into(),
                    color: Some("#FF2968".into()),
                    ctag: String::new(),
                },
                CachedCalendar { href: "/calendars/W/".into(), name: "Work".into(), color: None, ctag: String::new() },
            ],
            objects: vec![
                object(
                    "/calendars/P/",
                    "BEGIN:VEVENT\r\nUID:gym\r\nSUMMARY:Gym\r\nLOCATION:SATS\r\n\
                     DTSTART;TZID=Europe/Stockholm:20261023T070000\r\nDTEND;TZID=Europe/Stockholm:20261023T080000\r\n\
                     RRULE:FREQ=DAILY\r\nEND:VEVENT\r\n",
                ),
                object(
                    "/calendars/P/",
                    "BEGIN:VEVENT\r\nUID:trip\r\nSUMMARY:Trip\r\nDTSTART;VALUE=DATE:20261022\r\n\
                     DTEND;VALUE=DATE:20261025\r\nEND:VEVENT\r\n",
                ),
                object(
                    "/calendars/W/",
                    "BEGIN:VEVENT\r\nUID:standup\r\nSUMMARY:Standup\r\nDTSTART:20261024T080000Z\r\n\
                     DURATION:PT15M\r\nEND:VEVENT\r\n",
                ),
            ],
        }
    }

    fn lines(items: &[Item]) -> Vec<String> {
        items.iter().map(|i| format!("{} {} {}", &i.start[..16], i.title, i.calendar)).collect()
    }

    #[test]
    fn the_coming_days_by_day_with_all_day_events_first() {
        let items = agenda(&snapshot(), &[], stockholm(), day(2026, 10, 23), 2);
        assert_eq!(
            lines(&items),
            [
                "2026-10-22T00:00 Trip Personal",
                "2026-10-23T07:00 Gym Personal",
                "2026-10-24T07:00 Gym Personal",
                "2026-10-24T10:00 Standup Work",
            ]
        );
        assert!(items[0].all_day);
        assert_eq!(items[0].end, "2026-10-25T00:00:00+02:00");
        assert_eq!(items[1].location, "SATS");
        assert_eq!(items[1].color.as_deref(), Some("#FF2968"));
        assert_eq!(items[3].color, None);
    }

    #[test]
    fn hidden_calendars_are_left_out() {
        let items = agenda(&snapshot(), &["/calendars/P/".into()], stockholm(), day(2026, 10, 23), 2);
        assert_eq!(lines(&items), ["2026-10-24T10:00 Standup Work"]);
    }

    #[test]
    fn it_prints_as_json() {
        let items = agenda(&snapshot(), &[], stockholm(), day(2026, 10, 24), 1);
        let json = serde_json::to_value(&items[2]).unwrap();
        assert_eq!(json["uid"], "standup");
        assert_eq!(json["start"], "2026-10-24T10:00:00+02:00");
        assert_eq!(json["all_day"], false);
    }
}
