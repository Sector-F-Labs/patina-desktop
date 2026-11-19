//! Creating and editing events: the draft the event dialog edits, and
//! writing it into a calendar object. An edit changes only the fields the
//! dialog shows; every other property and component in the file is kept.

use chrono::{DateTime, Duration, NaiveDate, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;

use crate::ics::{self, Component, Prop, When};
use crate::model::Event;

/// What the event dialog edits.
#[derive(Debug, Clone, PartialEq)]
pub struct Draft {
    pub summary: String,
    pub location: String,
    pub description: String,
    pub calendar: String,
    pub all_day: bool,
    pub start_date: NaiveDate,
    pub start_time: NaiveTime,
    pub end_date: NaiveDate,
    pub end_time: NaiveTime,
}

impl Draft {
    /// A new one-hour event starting at `start`.
    pub fn new(calendar: &str, start: DateTime<Tz>, all_day: bool) -> Self {
        let end = start + Duration::hours(1);
        Self {
            summary: String::new(),
            location: String::new(),
            description: String::new(),
            calendar: calendar.to_string(),
            all_day,
            start_date: start.date_naive(),
            start_time: start.time(),
            end_date: if all_day { start.date_naive() } else { end.date_naive() },
            end_time: end.time(),
        }
    }

    /// The draft for an existing event. All-day events show their last day
    /// rather than the exclusive end.
    pub fn of(event: &Event, local: Tz) -> Self {
        let (start_date, start_time) = split(event.start, local);
        let (mut end_date, end_time) = split(event.end, local);
        if event.all_day() && end_date > start_date {
            end_date -= Duration::days(1);
        }
        Self {
            summary: event.summary.clone(),
            location: event.location.clone(),
            description: event.description.clone(),
            calendar: event.calendar.clone(),
            all_day: event.all_day(),
            start_date,
            start_time,
            end_date,
            end_time,
        }
    }

    pub fn start(&self, local: Tz) -> DateTime<Tz> {
        at(self.start_date, self.start_time, local)
    }

    pub fn end(&self, local: Tz) -> DateTime<Tz> {
        at(self.end_date, self.end_time, local)
    }

    /// What's wrong with the draft, if anything.
    pub fn problem(&self, local: Tz) -> Option<&'static str> {
        if self.all_day {
            (self.end_date < self.start_date).then_some("The event ends before it starts")
        } else {
            (self.end(local) < self.start(local)).then_some("The event ends before it starts")
        }
    }
}

fn split(w: When, local: Tz) -> (NaiveDate, NaiveTime) {
    match w {
        When::Date(d) => (d, NaiveTime::from_hms_opt(9, 0, 0).unwrap()),
        When::Time(t) => {
            let t = t.with_timezone(&local);
            (t.date_naive(), t.time())
        }
    }
}

fn at(day: NaiveDate, time: NaiveTime, tz: Tz) -> DateTime<Tz> {
    let naive = day.and_time(time);
    tz.from_local_datetime(&naive).earliest().unwrap_or_else(|| tz.from_utc_datetime(&naive))
}

fn now_stamp() -> String {
    Utc::now().format("%Y%m%dT%H%M%SZ").to_string()
}

/// A new calendar object holding the draft. Returns its file name and text.
pub fn create(draft: &Draft, local: Tz) -> (String, String) {
    let uid = uuid::Uuid::new_v4().to_string();
    let mut event = Component { name: "VEVENT".into(), ..Default::default() };
    event.set("UID", vec![], uid.clone());
    event.set("DTSTAMP", vec![], now_stamp());
    event.set("CREATED", vec![], now_stamp());
    apply(&mut event, draft, local, None);
    let cal = Component {
        name: "VCALENDAR".into(),
        props: vec![
            Prop { name: "VERSION".into(), params: vec![], value: "2.0".into() },
            Prop { name: "PRODID".into(), params: vec![], value: "-//Sector F Labs//Calendar//EN".into() },
        ],
        children: vec![event],
    };
    (format!("{uid}.ics"), ics::write(&[cal]))
}

/// The calendar object `text` with its `index`th event changed to the draft.
pub fn update(text: &str, index: usize, draft: &Draft, local: Tz) -> Option<String> {
    let mut top = ics::parse(text);
    let mut seen = 0;
    for cal in top.iter_mut().filter(|c| c.name == "VCALENDAR") {
        for comp in cal.children.iter_mut().filter(|c| c.name == "VEVENT") {
            if seen == index {
                let original = comp.get("DTSTART").cloned();
                apply(comp, draft, local, original.as_ref());
                let sequence = comp.get("SEQUENCE").and_then(|p| p.value.trim().parse::<u32>().ok()).unwrap_or(0);
                comp.set("SEQUENCE", vec![], (sequence + 1).to_string());
                comp.set("DTSTAMP", vec![], now_stamp());
                comp.set("LAST-MODIFIED", vec![], now_stamp());
                return Some(ics::write(&top));
            }
            seen += 1;
        }
    }
    None
}

/// Runs `f` on the `index`th VEVENT of `text` and writes the file back.
fn with_event(text: &str, index: usize, f: impl FnOnce(&mut Component)) -> Option<String> {
    let mut top = ics::parse(text);
    let mut seen = 0;
    for cal in top.iter_mut().filter(|c| c.name == "VCALENDAR") {
        for comp in cal.children.iter_mut().filter(|c| c.name == "VEVENT") {
            if seen == index {
                f(comp);
                comp.set("DTSTAMP", vec![], now_stamp());
                return Some(ics::write(&top));
            }
            seen += 1;
        }
    }
    None
}

/// Leaves one occurrence out of a recurring event, with an EXDATE written
/// the way its DTSTART is.
pub fn exclude(text: &str, index: usize, occurrence: DateTime<Tz>, local: Tz) -> Option<String> {
    with_event(text, index, |comp| {
        let Some(start) = comp.get("DTSTART").cloned() else { return };
        let (params, value) =
            if start.param("VALUE").is_some_and(|v| v.eq_ignore_ascii_case("DATE")) || start.value.len() == 8 {
                (
                    vec![("VALUE".to_string(), "DATE".to_string())],
                    ics::format_date(occurrence.with_timezone(&local).date_naive()),
                )
            } else if let Some(tz) = start.param("TZID").and_then(|z| ics::zone(z).map(|t| (z.to_string(), t))) {
                (vec![("TZID".to_string(), tz.0)], occurrence.with_timezone(&tz.1).format("%Y%m%dT%H%M%S").to_string())
            } else if start.value.ends_with('Z') {
                (vec![], ics::format_utc(occurrence))
            } else {
                (vec![], occurrence.with_timezone(&local).format("%Y%m%dT%H%M%S").to_string())
            };
        comp.props.push(Prop { name: "EXDATE".into(), params, value });
    })
}

/// The file without its `index`th VEVENT, or `None` when that was its only
/// event (delete the whole file then).
pub fn remove(text: &str, index: usize) -> Option<String> {
    let mut top = ics::parse(text);
    let events: usize = top
        .iter()
        .filter(|c| c.name == "VCALENDAR")
        .map(|c| c.children.iter().filter(|e| e.name == "VEVENT").count())
        .sum();
    if events <= 1 {
        return None;
    }
    let mut seen = 0;
    for cal in top.iter_mut().filter(|c| c.name == "VCALENDAR") {
        cal.children.retain(|comp| {
            if comp.name != "VEVENT" {
                return true;
            }
            seen += 1;
            seen - 1 != index
        });
    }
    Some(ics::write(&top))
}

/// Writes the draft's fields into a VEVENT. Timed events keep the way the
/// original start was written (in a named zone, floating, or UTC), so a
/// weekly event in Stockholm stays at the same local time across summer
/// time; new ones are written in UTC.
fn apply(event: &mut Component, draft: &Draft, local: Tz, original: Option<&Prop>) {
    event.set("SUMMARY", vec![], ics::escape(draft.summary.trim()));
    for (name, value) in [("LOCATION", &draft.location), ("DESCRIPTION", &draft.description)] {
        if value.trim().is_empty() {
            event.remove(name);
        } else {
            event.set(name, vec![], ics::escape(value.trim()));
        }
    }
    event.remove("DURATION");
    let date = || vec![("VALUE".to_string(), "DATE".to_string())];
    if draft.all_day {
        event.set("DTSTART", date(), ics::format_date(draft.start_date));
        event.set("DTEND", date(), ics::format_date(draft.end_date + Duration::days(1)));
        return;
    }
    let (start, end) = (draft.start(local), draft.end(local));
    let zoned = original.and_then(|p| p.param("TZID").map(|z| (z.to_string(), ics::zone(z))));
    let floating = original.is_some_and(|p| p.param("TZID").is_none() && !p.value.ends_with('Z') && p.value.len() > 8);
    let write = |t: DateTime<Tz>| -> (Vec<(String, String)>, String) {
        match &zoned {
            Some((name, Some(tz))) => {
                (vec![("TZID".into(), name.clone())], t.with_timezone(tz).format("%Y%m%dT%H%M%S").to_string())
            }
            _ if floating => (vec![], t.with_timezone(&local).format("%Y%m%dT%H%M%S").to_string()),
            _ => (vec![], ics::format_utc(t)),
        }
    };
    let (params, value) = write(start);
    event.set("DTSTART", params, value);
    let (params, value) = write(end);
    event.set("DTEND", params, value);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::events_from_ics;

    fn stockholm() -> Tz {
        "Europe/Stockholm".parse().unwrap()
    }

    fn draft() -> Draft {
        Draft {
            summary: "Lunch, maybe".into(),
            location: "Café".into(),
            description: String::new(),
            calendar: "/calendars/Personal/".into(),
            all_day: false,
            start_date: NaiveDate::from_ymd_opt(2026, 10, 12).unwrap(),
            start_time: NaiveTime::from_hms_opt(12, 0, 0).unwrap(),
            end_date: NaiveDate::from_ymd_opt(2026, 10, 12).unwrap(),
            end_time: NaiveTime::from_hms_opt(13, 0, 0).unwrap(),
        }
    }

    #[test]
    fn a_new_event_reads_back_as_drafted() {
        let (file, text) = create(&draft(), stockholm());
        assert!(file.ends_with(".ics"));
        assert!(text.contains("DTSTART:20261012T100000Z"), "{text}");
        let events = events_from_ics("/calendars/Personal/", &file, None, &text, stockholm());
        assert_eq!(Draft::of(&events[0], stockholm()), draft());
    }

    #[test]
    fn all_day_events_store_an_exclusive_end() {
        let mut d = draft();
        d.all_day = true;
        d.end_date = d.start_date + Duration::days(2);
        let (_, text) = create(&d, stockholm());
        assert!(text.contains("DTEND;VALUE=DATE:20261015"), "{text}");
        let events = events_from_ics("c", "h", None, &text, stockholm());
        let back = Draft::of(&events[0], stockholm());
        assert_eq!((back.start_date, back.end_date), (d.start_date, d.end_date));
    }

    #[test]
    fn editing_keeps_other_properties_zone_and_rule() {
        let text = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VTIMEZONE\r\nTZID:Europe/Stockholm\r\nEND:VTIMEZONE\r\n\
                    BEGIN:VEVENT\r\nUID:gym\r\nSUMMARY:Gym\r\nDTSTART;TZID=Europe/Stockholm:20261005T070000\r\n\
                    DURATION:PT1H\r\nRRULE:FREQ=WEEKLY;BYDAY=MO\r\nX-APPLE-STRUCTURED-LOCATION:keep\r\nSEQUENCE:2\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let events = events_from_ics("c", "h", None, text, stockholm());
        let mut d = Draft::of(&events[0], stockholm());
        d.summary = "Gym (heavy)".into();
        d.start_time = NaiveTime::from_hms_opt(7, 30, 0).unwrap();
        d.end_time = NaiveTime::from_hms_opt(8, 45, 0).unwrap();
        let out = update(text, 0, &d, stockholm()).unwrap();
        assert!(out.contains("DTSTART;TZID=Europe/Stockholm:20261005T073000"), "{out}");
        assert!(out.contains("DTEND;TZID=Europe/Stockholm:20261005T084500"));
        assert!(out.contains("RRULE:FREQ=WEEKLY;BYDAY=MO"));
        assert!(out.contains("X-APPLE-STRUCTURED-LOCATION:keep"));
        assert!(out.contains("BEGIN:VTIMEZONE"));
        assert!(out.contains("SEQUENCE:3"));
        assert!(!out.contains("DURATION"), "DTEND replaces DURATION");
        assert!(out.contains("SUMMARY:Gym (heavy)"));
    }

    #[test]
    fn excluding_one_occurrence_of_a_zoned_series() {
        let text = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:gym\r\nDTSTART;TZID=Europe/Stockholm:20261005T070000\r\nDTEND;TZID=Europe/Stockholm:20261005T080000\r\nRRULE:FREQ=WEEKLY\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let tz = stockholm();
        let occ = tz.with_ymd_and_hms(2026, 10, 12, 7, 0, 0).unwrap();
        let out = exclude(text, 0, occ, tz).unwrap();
        assert!(out.contains("EXDATE;TZID=Europe/Stockholm:20261012T070000"), "{out}");
        let events = events_from_ics("c", "h", None, &out, tz);
        let day = |d| NaiveDate::from_ymd_opt(2026, 10, d).unwrap();
        let left = crate::model::occurrences(&events, day(5), day(19), tz, |_| true);
        assert_eq!(left.len(), 2, "the 12th is gone, the 5th and 19th stay");
    }

    #[test]
    fn removing_the_only_event_means_deleting_the_file() {
        let one = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:a\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        assert_eq!(remove(one, 0), None);
        let two = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:a\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:a\r\nRECURRENCE-ID:20261012T080000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let out = remove(two, 1).unwrap();
        assert!(!out.contains("RECURRENCE-ID"));
        assert!(out.contains("UID:a"));
    }

    #[test]
    fn a_draft_that_ends_before_it_starts_is_a_problem() {
        let mut d = draft();
        d.end_time = NaiveTime::from_hms_opt(11, 0, 0).unwrap();
        assert!(d.problem(stockholm()).is_some());
    }
}
