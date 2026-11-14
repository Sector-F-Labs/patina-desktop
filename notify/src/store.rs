//! The notifications on show, as plain data with no bus or surface in it
//! (ADR-0141): what came in, what replaced what, and what has run out of time.

use std::time::{Duration, Instant};

/// Why a notification closed, as the specification numbers them.
pub const EXPIRED: u32 = 1;
pub const DISMISSED: u32 = 2;
pub const CLOSED_BY_CALL: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Urgency {
    Low,
    Normal,
    Critical,
}

impl Urgency {
    pub fn from_hint(byte: Option<u8>) -> Urgency {
        match byte {
            Some(0) => Urgency::Low,
            Some(2) => Urgency::Critical,
            _ => Urgency::Normal,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    pub id: u32,
    pub app: String,
    /// The `desktop-entry` hint: the app's desktop file, without `.desktop`.
    pub desktop_entry: String,
    pub summary: String,
    pub body: String,
    /// (key, label) of each action; "default" is the card being clicked.
    pub actions: Vec<(String, String)>,
    pub urgency: Urgency,
    pub expires: Option<Instant>,
}

#[derive(Default)]
pub struct Store {
    notes: Vec<Note>,
    last: u32,
}

impl Store {
    pub fn notes(&self) -> &[Note] {
        &self.notes
    }

    pub fn is_empty(&self) -> bool {
        self.notes.is_empty()
    }

    /// Adds `note`, or puts it in place of the one at `replaces` (keeping its
    /// place in the column); returns its id. `timeout_ms` is the app's: 0 and
    /// -1 mean it stays, as does anything critical.
    pub fn notify(&mut self, replaces: u32, mut note: Note, timeout_ms: i32, now: Instant) -> u32 {
        note.expires = (timeout_ms > 0 && note.urgency != Urgency::Critical)
            .then(|| now + Duration::from_millis(timeout_ms as u64));
        if let Some(at) = self.notes.iter().position(|n| n.id == replaces) {
            note.id = replaces;
            self.notes[at] = note;
            return replaces;
        }
        note.id = self.next_id();
        let id = note.id;
        self.notes.push(note);
        id
    }

    fn next_id(&mut self) -> u32 {
        loop {
            self.last = self.last.checked_add(1).unwrap_or(1);
            if !self.notes.iter().any(|n| n.id == self.last) {
                return self.last;
            }
        }
    }

    /// Removes the notification, if it's there.
    pub fn close(&mut self, id: u32) -> Option<Note> {
        let at = self.notes.iter().position(|n| n.id == id)?;
        Some(self.notes.remove(at))
    }

    /// Removes and returns the ids of those whose time has run out.
    pub fn expire(&mut self, now: Instant) -> Vec<u32> {
        let gone: Vec<u32> = self
            .notes
            .iter()
            .filter(|n| n.expires.is_some_and(|t| t <= now))
            .map(|n| n.id)
            .collect();
        self.notes.retain(|n| !gone.contains(&n.id));
        gone
    }

    /// When the next one runs out, if any will.
    pub fn next_expiry(&self) -> Option<Instant> {
        self.notes.iter().filter_map(|n| n.expires).min()
    }
}

/// `text` as letters and digits only, lowercase, to compare names that are
/// spelt differently ("Telegram Desktop", "telegram-desktop_telegram-desktop").
fn norm(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// The window of the app that sent `note`, from `windows` (id, app id): the one
/// whose app id contains the desktop entry or the app's name, or the other way
/// round. Names under three letters are too vague to go by.
pub fn window_for(note: &Note, windows: &[(u64, String)]) -> Option<u64> {
    [&note.desktop_entry, &note.app]
        .into_iter()
        .map(|name| norm(name))
        .filter(|name| name.len() >= 3)
        .find_map(|name| {
            windows.iter().find_map(|(id, app_id)| {
                let app_id = norm(app_id);
                (app_id.len() >= 3 && (app_id.contains(&name) || name.contains(&app_id)))
                    .then_some(*id)
            })
        })
}

/// A body's text without the markup the specification allows (`<b>`, `<i>`,
/// `<u>`, `<a href>`, `<img>`), with its entities undone.
pub fn plain(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_tag = false;
    for c in text.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// Pairs the flat list an app sends (`key, label, key, label`) up.
pub fn action_pairs(flat: &[String]) -> Vec<(String, String)> {
    flat.chunks_exact(2)
        .map(|p| (p[0].clone(), p[1].clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(summary: &str) -> Note {
        Note {
            id: 0,
            app: "app".into(),
            desktop_entry: String::new(),
            summary: summary.into(),
            body: String::new(),
            actions: Vec::new(),
            urgency: Urgency::Normal,
            expires: None,
        }
    }

    #[test]
    fn notifications_stack_in_the_order_they_came_with_ids_from_one() {
        let (mut s, now) = (Store::default(), Instant::now());
        assert_eq!(s.notify(0, note("a"), -1, now), 1);
        assert_eq!(s.notify(0, note("b"), -1, now), 2);
        let summaries: Vec<_> = s.notes().iter().map(|n| n.summary.as_str()).collect();
        assert_eq!(summaries, ["a", "b"]);
    }

    #[test]
    fn a_replacement_keeps_its_id_and_its_place() {
        let (mut s, now) = (Store::default(), Instant::now());
        s.notify(0, note("a"), -1, now);
        s.notify(0, note("b"), -1, now);
        assert_eq!(s.notify(1, note("a, again"), -1, now), 1);
        assert_eq!(s.notes()[0].summary, "a, again");
        assert_eq!(s.notes().len(), 2);
    }

    #[test]
    fn replacing_one_that_is_gone_makes_a_new_one() {
        let (mut s, now) = (Store::default(), Instant::now());
        s.notify(0, note("a"), -1, now);
        s.close(1);
        let id = s.notify(1, note("b"), -1, now);
        assert_ne!(id, 1);
        assert_eq!(s.notes().len(), 1);
    }

    #[test]
    fn ids_are_not_reused_while_in_use() {
        let mut s = Store {
            last: u32::MAX,
            ..Store::default()
        };
        let now = Instant::now();
        s.notify(0, note("a"), -1, now);
        s.notify(0, note("b"), -1, now);
        let ids: Vec<_> = s.notes().iter().map(|n| n.id).collect();
        assert_eq!(ids, [1, 2]);
    }

    #[test]
    fn only_a_timeout_makes_one_expire_and_critical_ones_never_do() {
        let (mut s, now) = (Store::default(), Instant::now());
        s.notify(0, note("stays"), -1, now);
        s.notify(0, note("never"), 0, now);
        s.notify(0, note("goes"), 1000, now);
        let mut critical = note("critical");
        critical.urgency = Urgency::Critical;
        s.notify(0, critical, 1000, now);
        assert_eq!(s.next_expiry(), Some(now + Duration::from_secs(1)));
        assert!(s.expire(now + Duration::from_millis(999)).is_empty());
        assert_eq!(s.expire(now + Duration::from_secs(1)), [3]);
        assert_eq!(s.notes().len(), 3);
        assert_eq!(s.next_expiry(), None);
    }

    #[test]
    fn closing_returns_what_was_closed_once() {
        let (mut s, now) = (Store::default(), Instant::now());
        s.notify(0, note("a"), -1, now);
        assert_eq!(s.close(1).map(|n| n.summary), Some("a".to_string()));
        assert!(s.close(1).is_none());
        assert!(s.is_empty());
    }

    #[test]
    fn markup_is_taken_out_of_a_body() {
        assert_eq!(
            plain("<b>15</b> updates &amp; <i>more</i>"),
            "15 updates & more"
        );
        assert_eq!(plain("1 &lt; 2"), "1 < 2");
        assert_eq!(plain("<a href=\"x\">link</a>"), "link");
    }

    #[test]
    fn an_apps_window_is_found_by_its_entry_or_its_name() {
        let windows = vec![
            (1, "com.sectorflabs.Settings".to_string()),
            (2, "telegram-desktop_telegram-desktop".to_string()),
        ];
        let mut n = note("hi");
        n.app = "Telegram Desktop".into();
        assert_eq!(window_for(&n, &windows), Some(2));
        n.app = "something else".into();
        n.desktop_entry = "com.sectorflabs.Settings".into();
        assert_eq!(window_for(&n, &windows), Some(1));
        n.desktop_entry.clear();
        assert_eq!(window_for(&n, &windows), None);
        n.app = "me".into();
        assert_eq!(window_for(&n, &windows), None, "too short to go by");
    }

    #[test]
    fn actions_come_in_pairs() {
        let flat = ["default".to_string(), "Open".into(), "dangling".into()];
        assert_eq!(
            action_pairs(&flat),
            [("default".to_string(), "Open".to_string())]
        );
    }

    #[test]
    fn urgency_comes_from_the_hint() {
        assert_eq!(Urgency::from_hint(None), Urgency::Normal);
        assert_eq!(Urgency::from_hint(Some(0)), Urgency::Low);
        assert_eq!(Urgency::from_hint(Some(2)), Urgency::Critical);
        assert_eq!(Urgency::from_hint(Some(9)), Urgency::Normal);
    }
}
