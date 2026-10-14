//! dwl's status output. dwl's `printstatus()` writes one line per field,
//! per monitor, after every change, to the stdin of its `-s` command:
//!
//! ```text
//! eDP-1 title ~/Src — nvim
//! eDP-1 appid foot
//! eDP-1 fullscreen 0
//! eDP-1 floating 0
//! eDP-1 selmon 1
//! eDP-1 tags 7 1 1 0
//! eDP-1 layout []=
//! ```
//!
//! `tags` is four bitmasks: occupied, selected, the focused client's and
//! urgent. With no focused client, title, appid, fullscreen and floating are
//! empty. Lines this parser doesn't know (from patched dwls) are ignored.

use std::collections::BTreeMap;

/// What dwl last said about one monitor.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Monitor {
    pub title: String,
    pub appid: String,
    pub fullscreen: bool,
    pub floating: bool,
    pub selected: bool,
    pub tags: Tags,
    pub layout: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tags {
    pub occupied: u32,
    pub selected: u32,
    pub client: u32,
    pub urgent: u32,
}

/// Every monitor dwl has reported, by output name (e.g. `HEADLESS-1`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Status {
    pub monitors: BTreeMap<String, Monitor>,
}

impl Status {
    /// Applies one status line. Returns whether anything changed.
    pub fn apply(&mut self, line: &str) -> bool {
        let line = line.strip_suffix('\n').unwrap_or(line);
        let mut parts = line.splitn(3, ' ');
        let (Some(output), Some(field)) = (parts.next(), parts.next()) else {
            return false;
        };
        if output.is_empty() {
            return false;
        }
        let value = parts.next().unwrap_or("");
        let mut m = self.monitors.get(output).cloned().unwrap_or_default();
        match field {
            "title" => m.title = value.to_string(),
            "appid" => m.appid = value.to_string(),
            "fullscreen" => m.fullscreen = value == "1",
            "floating" => m.floating = value == "1",
            "selmon" => m.selected = value == "1",
            "layout" => m.layout = value.to_string(),
            "tags" => {
                let n: Vec<u32> = value.split(' ').filter_map(|n| n.parse().ok()).collect();
                let [occupied, selected, client, urgent] = n[..] else {
                    return false;
                };
                m.tags = Tags {
                    occupied,
                    selected,
                    client,
                    urgent,
                };
            }
            _ => return false,
        }
        let changed = self.monitors.get(output) != Some(&m);
        self.monitors.insert(output.to_string(), m);
        changed
    }

    /// The monitor dwl last marked as selected.
    pub fn selected(&self) -> Option<(&str, &Monitor)> {
        self.monitors
            .iter()
            .find(|(_, m)| m.selected)
            .map(|(n, m)| (n.as_str(), m))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_full_report() {
        let mut s = Status::default();
        for line in [
            "eDP-1 title ~/Src — nvim",
            "eDP-1 appid foot",
            "eDP-1 fullscreen 0",
            "eDP-1 floating 1",
            "eDP-1 selmon 1",
            "eDP-1 tags 7 1 1 4",
            "eDP-1 layout []=",
        ] {
            s.apply(line);
        }
        let m = &s.monitors["eDP-1"];
        assert_eq!(m.title, "~/Src — nvim");
        assert_eq!(m.appid, "foot");
        assert!(!m.fullscreen);
        assert!(m.floating);
        assert!(m.selected);
        assert_eq!(
            m.tags,
            Tags {
                occupied: 7,
                selected: 1,
                client: 1,
                urgent: 4
            }
        );
        assert_eq!(m.layout, "[]=");
        assert_eq!(s.selected().map(|(n, _)| n), Some("eDP-1"));
    }

    #[test]
    fn empty_fields_when_nothing_is_focused() {
        let mut s = Status::default();
        s.apply("HEADLESS-1 title foo");
        assert!(s.apply("HEADLESS-1 title "));
        assert!(!s.apply("HEADLESS-1 fullscreen "));
        assert_eq!(s.monitors["HEADLESS-1"].title, "");
    }

    #[test]
    fn titles_keep_their_spaces() {
        let mut s = Status::default();
        s.apply("DP-2 title  two  spaces ");
        assert_eq!(s.monitors["DP-2"].title, " two  spaces ");
    }

    #[test]
    fn repeats_are_not_changes() {
        let mut s = Status::default();
        assert!(s.apply("DP-1 layout [M]\n"));
        assert!(!s.apply("DP-1 layout [M]"));
    }

    #[test]
    fn ignores_unknown_and_broken_lines() {
        let mut s = Status::default();
        assert!(!s.apply(""));
        assert!(!s.apply("DP-1"));
        assert!(!s.apply("DP-1 kb_layout us"));
        assert!(!s.apply("DP-1 tags 1 2"));
        assert!(s.monitors.is_empty());
    }
}
