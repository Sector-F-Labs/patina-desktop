//! The events, from the Calendar app: `calendar --agenda` prints the days
//! asked for as JSON, from what it last synced (Calendar's ADR-0009), and
//! `calendar --open` opens it at a day or an event. The menu draws them;
//! Calendar keeps the data.

use std::collections::BTreeSet;

use chrono::{DateTime, Duration, FixedOffset, Local, NaiveDate};
use serde::Deserialize;

/// The Calendar app's command.
pub const CALENDAR: &str = "calendar";

/// One occurrence, as `calendar --agenda` prints it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Item {
    pub uid: String,
    pub title: String,
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub calendar: String,
    pub color: Option<String>,
    pub start: DateTime<FixedOffset>,
    /// Exclusive.
    pub end: DateTime<FixedOffset>,
    pub all_day: bool,
}

impl Item {
    /// The local days it covers, inclusive: an end at midnight belongs to
    /// the day before.
    pub fn days(&self) -> (NaiveDate, NaiveDate) {
        let first = self.start.with_timezone(&Local).date_naive();
        let last = if self.end > self.start {
            self.end - Duration::seconds(1)
        } else {
            self.end
        };
        (first, last.with_timezone(&Local).date_naive().max(first))
    }

    pub fn on(&self, day: NaiveDate) -> bool {
        let (first, last) = self.days();
        (first..=last).contains(&day)
    }

    /// What its row says for the time on `day`: "All day", "09:30", or
    /// "until 11:00" for one begun the day before.
    pub fn when(&self, day: NaiveDate) -> String {
        let (first, last) = self.days();
        let time = |t: DateTime<FixedOffset>| t.with_timezone(&Local).format("%H:%M").to_string();
        if self.all_day || (first < day && last > day) {
            "All day".into()
        } else if first < day {
            format!("until {}", time(self.end))
        } else {
            time(self.start)
        }
    }
}

/// The days that have something on, among `items`.
pub fn busy_days(items: &[Item]) -> BTreeSet<NaiveDate> {
    let mut days = BTreeSet::new();
    for item in items {
        let (first, last) = item.days();
        // A long event marks its days, but not a year of them.
        for day in first.iter_days().take(62).take_while(|d| *d <= last) {
            days.insert(day);
        }
    }
    days
}

/// `days` days from `from`, by running `calendar --agenda`. An error says
/// why there are none to show, such as Calendar not being installed.
pub fn read(from: NaiveDate, days: u32) -> Result<Vec<Item>, String> {
    let out = std::process::Command::new(CALENDAR)
        .args([
            "--agenda",
            "--from",
            &from.to_string(),
            "--days",
            &days.to_string(),
        ])
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => "Calendar isn't installed".to_string(),
            _ => format!("can't run {CALENDAR}: {e}"),
        })?;
    if !out.status.success() {
        let why = String::from_utf8_lossy(&out.stderr);
        return Err(format!("{CALENDAR} --agenda failed: {}", why.trim()));
    }
    parse(&String::from_utf8_lossy(&out.stdout))
}

pub fn parse(json: &str) -> Result<Vec<Item>, String> {
    serde_json::from_str(json).map_err(|e| format!("can't read Calendar's agenda: {e}"))
}

/// The command line that opens Calendar at `day`, and at `event`'s
/// occurrence on it.
pub fn open_command(day: NaiveDate, event: Option<&str>) -> String {
    let mut line = format!("{CALENDAR} --open {day}");
    if let Some(uid) = event {
        line.push_str(" --event ");
        line.push_str(&quote(uid));
    }
    line
}

/// `text` as one word to sh(1).
fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// The colour in `#RRGGBB` or `#RRGGBBAA`, as CalDAV servers give them.
pub fn colour(hex: &str) -> Option<egui::Color32> {
    let hex = hex.strip_prefix('#')?;
    let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
    match hex.len() {
        6 | 8 => Some(egui::Color32::from_rgb(byte(0)?, byte(2)?, byte(4)?)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What Calendar printed for a day with a trip, the market and sunrise.
    const AGENDA: &str = r##"[
      {"uid": "trip", "title": "Trip", "location": "", "calendar": "Personal", "color": "#FF2968",
       "start": "2026-10-22T00:00:00+02:00", "end": "2026-10-25T00:00:00+02:00", "all_day": true},
      {"uid": "sun-1@plan-sun", "title": "Sunrise", "location": "", "calendar": "Sun", "color": "#FF8C00",
       "start": "2026-10-24T07:47:03+02:00", "end": "2026-10-24T07:52:03+02:00", "all_day": false},
      {"uid": "AC02", "title": "Ulriksdals Höstmarknad", "location": "Ulriksdals slott", "calendar": "Blocks",
       "color": null, "start": "2026-10-24T11:00:00+02:00", "end": "2026-10-25T15:00:00+01:00", "all_day": false}
    ]"##;

    fn day(m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, m, d).unwrap()
    }

    #[test]
    fn calendars_agenda_reads_back() {
        let items = parse(AGENDA).unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[2].location, "Ulriksdals slott");
        assert_eq!(items[2].color, None);
        assert!(parse("not json").is_err());
    }

    /// These read times in the local zone, so they hold where it's within
    /// an hour or two of Stockholm, as the machines here are.
    #[test]
    fn an_all_day_event_ends_the_day_before_its_end() {
        let items = parse(AGENDA).unwrap();
        assert_eq!(items[0].days(), (day(10, 22), day(10, 24)));
        assert!(items[0].on(day(10, 24)));
        assert!(!items[0].on(day(10, 25)));
        assert_eq!(items[0].when(day(10, 23)), "All day");
        let busy = busy_days(&items);
        assert!(busy.contains(&day(10, 22)) && busy.contains(&day(10, 25)));
        assert!(!busy.contains(&day(10, 26)));
    }

    #[test]
    fn a_long_event_reads_until_its_end_on_its_last_day() {
        let items = parse(AGENDA).unwrap();
        assert!(items[2].when(day(10, 24)).contains(':'));
        assert!(items[2].when(day(10, 25)).starts_with("until "));
    }

    #[test]
    fn opening_an_event_quotes_its_uid() {
        assert_eq!(
            open_command(day(10, 24), None),
            "calendar --open 2026-10-24"
        );
        assert_eq!(
            open_command(day(10, 24), Some("it's@x")),
            r"calendar --open 2026-10-24 --event 'it'\''s@x'"
        );
    }

    #[test]
    fn caldav_colours() {
        assert_eq!(
            colour("#FF8C00"),
            Some(egui::Color32::from_rgb(255, 140, 0))
        );
        assert_eq!(
            colour("#FF8C00FF"),
            Some(egui::Color32::from_rgb(255, 140, 0))
        );
        assert_eq!(colour("orange"), None);
    }
}
