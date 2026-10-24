//! What looks wrong, for the top of the Overview (ADR-0090): programs
//! waiting on the CPU, memory or disks over the last minute; since the
//! machine started, the programs that crashed, were killed for memory or
//! hung, and errors from the disks, the GPU and the heat, each kind once
//! with how often; and processes that exited but weren't cleaned up. Each
//! leads to where it can be looked into.

use std::collections::BTreeMap;

use crate::kernel_log::{Entry, Event};
use crate::log_view::when;
use crate::pressure::Kind;
use crate::processes::Model;
use crate::sample::State;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Warning,
    Error,
}

/// Where a problem leads.
#[derive(Debug, Clone, PartialEq)]
pub enum Go {
    /// The Log tab, showing this kind of event, with these words.
    Log(Option<&'static str>, String),
    /// Performance's Pressure page.
    Pressure,
    Nowhere,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Problem {
    pub severity: Severity,
    pub title: String,
    pub detail: String,
    pub go: Go,
    /// What it means, in a few sentences.
    pub meaning: &'static str,
    /// When it last happened, for the order; now for what's going on.
    latest: f64,
}

const PRESSURE: &str = "Pressure is the share of time programs were ready to run but had \
    to wait: for the CPU, for memory (being freed or read back from swap) or for the disks. \
    Unlike how busy something is, it says the machine is holding programs up, which is when \
    it feels slow or stuck. \"Every program at once\" is the share when nothing could run.";

const ZOMBIES: &str = "When a program ends, it stays listed until the program that started \
    it collects how it ended. These are waiting for that. They use no CPU or memory, but many \
    of them mean the parent is stuck or has a bug.";

/// How long ago a problem can last have happened and still be one to
/// solve now, rather than an older issue: a day.
const RECENT: f64 = 24.0 * 60.0 * 60.0;

impl Problem {
    /// Whether it's going on, or last happened within the day before `now`
    /// (seconds since the epoch). One whose time isn't known is older.
    pub fn recent(&self, now: f64) -> bool {
        self.latest > 0.0 && self.latest >= now - RECENT
    }
}

pub fn find(model: &Model, log: &[Entry]) -> Vec<Problem> {
    let mut found = Vec::new();
    if let Some(p) = &model.snapshot.pressure {
        for kind in Kind::ALL {
            let stall = p.of(kind);
            if stall.some[1] < kind.worrying() {
                continue;
            }
            let all = match stall.full {
                Some(full) if kind != Kind::Cpu && full[1] >= 1.0 => {
                    format!("; every program at once, {:.0}%", full[1])
                }
                _ => String::new(),
            };
            found.push(Problem {
                severity: Severity::Warning,
                title: format!(
                    "Programs waited for {} {:.0}% of the last minute",
                    kind.waiting_for(),
                    stall.some[1]
                ),
                detail: format!("{:.0}% over five minutes{all}", stall.some[2]),
                go: Go::Pressure,
                meaning: PRESSURE,
                latest: f64::MAX,
            });
        }
    }

    // Each kind once, the programs by name: (count, latest entry).
    let mut by: BTreeMap<(&'static str, String), (usize, &Entry)> = BTreeMap::new();
    for e in log {
        let Some(event) = &e.event else {
            continue;
        };
        let name = event
            .process()
            .map_or(String::new(), |(n, _)| n.to_string());
        let seen = by.entry((event.label(), name)).or_insert((0, e));
        seen.0 += 1;
        seen.1 = e;
    }
    for ((label, name), (count, latest)) in by {
        let times = match count {
            1 => String::new(),
            n => format!(" {n} times"),
        };
        let event = latest.event.as_ref().expect("an event");
        let (severity, title) = match event {
            Event::Crash { .. } => (Severity::Error, format!("{name} crashed{times}")),
            Event::OutOfMemory { .. } => (
                Severity::Error,
                format!("The kernel ended {name} for want of memory{times}"),
            ),
            Event::Hung { .. } => (
                Severity::Warning,
                format!("{name} was stuck in the kernel for minutes{times}"),
            ),
            Event::Disk => (Severity::Error, plural(count, "disk error")),
            Event::Gpu => (Severity::Warning, plural(count, "GPU error")),
            Event::Heat => (Severity::Warning, format!("Ran too hot{times}")),
            Event::Error => (Severity::Warning, plural(count, "other kernel error")),
        };
        let what = match event {
            Event::Crash { .. } => {
                crashed_in(&latest.message).map_or(String::new(), |lib| format!(", in {lib}"))
            }
            Event::Disk | Event::Gpu | Event::Heat | Event::Error => {
                format!(": {}", shorten(&latest.message, 90))
            }
            _ => String::new(),
        };
        let at = when(latest.time);
        let detail = if at.is_empty() {
            what.trim_start_matches([',', ':', ' ']).to_string()
        } else {
            format!("Last {at}{what}")
        };
        found.push(Problem {
            severity,
            title,
            detail,
            go: Go::Log(Some(label), name),
            meaning: crate::kernel_log::meaning(label),
            latest: latest.time,
        });
    }

    let zombies: Vec<&str> = model
        .snapshot
        .processes
        .iter()
        .filter(|p| p.state == State::Zombie)
        .map(|p| p.name.as_str())
        .collect();
    if !zombies.is_empty() {
        let mut names = zombies.clone();
        names.sort_unstable();
        names.dedup();
        found.push(Problem {
            severity: Severity::Warning,
            title: format!(
                "{} exited but {} cleaned up by {} parent",
                plural(zombies.len(), "process"),
                if zombies.len() == 1 {
                    "wasn't"
                } else {
                    "weren't"
                },
                if zombies.len() == 1 { "its" } else { "their" }
            ),
            detail: shorten(&names.join(", "), 90),
            go: Go::Nowhere,
            meaning: ZOMBIES,
            latest: f64::MAX,
        });
    }

    found.sort_by(|a, b| {
        b.severity
            .cmp(&a.severity)
            .then(b.latest.total_cmp(&a.latest))
    });
    found
}

fn plural(n: usize, what: &str) -> String {
    match (n, what.ends_with('s')) {
        (1, _) => format!("1 {what}"),
        (_, true) => format!("{n} {what}es"),
        _ => format!("{n} {what}s"),
    }
}

/// The library a segfault was in: "… error 6 in libxul.so[606ae0a,…]".
fn crashed_in(message: &str) -> Option<&str> {
    let (_, rest) = message.rsplit_once(" in ")?;
    let lib = rest.split('[').next()?.trim();
    (!lib.is_empty()).then_some(lib)
}

fn shorten(text: &str, most: usize) -> String {
    match text.char_indices().nth(most) {
        Some((i, _)) => format!("{}…", &text[..i]),
        None => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel_log::classify;
    use crate::pressure::{Pressure, Stall};
    use crate::processes::Model;

    fn entry(time: f64, priority: u8, message: &str) -> Entry {
        Entry {
            time,
            priority,
            message: message.into(),
            event: classify(message, priority),
        }
    }

    fn model() -> Model {
        Model::new(crate::processes::tests::snapshot())
    }

    #[test]
    fn crashes_of_one_program_are_one_problem_with_where() {
        let log = [
            entry(
                1.0,
                6,
                "drag-out[1]: segfault at 0 ip 1 sp 2 error 6 in libwayland-client.so.0.24.0[90b1,7+7]",
            ),
            entry(
                5.0,
                6,
                "drag-out[2]: segfault at 0 ip 1 sp 2 error 6 in libwayland-client.so.0.24.0[90b1,7+7]",
            ),
            entry(3.0, 6, "usb 1-1: new device"),
        ];
        let found = find(&model(), &log);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].title, "drag-out crashed 2 times");
        assert!(
            found[0]
                .detail
                .ends_with(", in libwayland-client.so.0.24.0"),
            "{}",
            found[0].detail
        );
        assert_eq!(found[0].go, Go::Log(Some("Crash"), "drag-out".into()));
    }

    #[test]
    fn errors_come_before_warnings_and_pressure_warns() {
        let mut m = model();
        m.snapshot.pressure = Some(Pressure {
            io: Stall {
                some: [5.0, 12.0, 9.0],
                full: Some([4.0, 8.0, 7.0]),
            },
            ..Default::default()
        });
        let log = [entry(9.0, 3, "[drm] *ERROR* nvidia-drm Failed to map")];
        let found = find(&m, &log);
        let titles: Vec<&str> = found.iter().map(|p| p.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "Programs waited for the disks 12% of the last minute",
                "1 GPU error"
            ]
        );
        assert_eq!(
            found[0].detail,
            "9% over five minutes; every program at once, 8%"
        );
        let disk = [entry(1.0, 3, "blk_update_request: I/O error, dev sda")];
        assert_eq!(find(&m, &disk)[0].title, "1 disk error");
    }

    #[test]
    fn a_problem_last_seen_a_day_ago_is_an_older_issue() {
        let now = 1_000_000.0;
        let log = [
            entry(
                now - 3600.0,
                6,
                "a[1]: segfault at 0 ip 1 sp 2 error 6 in x.so[1,2+3]",
            ),
            entry(
                now - 2.0 * RECENT,
                6,
                "b[2]: segfault at 0 ip 1 sp 2 error 6 in x.so[1,2+3]",
            ),
            entry(0.0, 3, "blk_update_request: I/O error, dev sda"),
        ];
        let mut m = model();
        m.snapshot.pressure = Some(Pressure {
            cpu: Stall {
                some: [50.0, 50.0, 50.0],
                full: None,
            },
            ..Default::default()
        });
        let found = find(&m, &log);
        let recent: Vec<&str> = found
            .iter()
            .filter(|p| p.recent(now))
            .map(|p| p.title.as_str())
            .collect();
        assert_eq!(
            recent,
            [
                "a crashed",
                "Programs waited for the CPU 50% of the last minute"
            ]
        );
        assert_eq!(found.iter().filter(|p| !p.recent(now)).count(), 2);
    }

    #[test]
    fn plurals() {
        assert_eq!(plural(1, "process"), "1 process");
        assert_eq!(plural(3, "process"), "3 processes");
        assert_eq!(plural(2, "GPU error"), "2 GPU errors");
    }
}
