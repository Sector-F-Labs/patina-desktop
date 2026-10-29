//! The battery from Linux's /sys/class/power_supply. Plain file reads, cheap
//! enough to repeat every few seconds.

use std::path::Path;

use super::{Battery, Charge, Health, PowerSource};

/// One battery's readings, in µWh and µW (or µAh and µA; the ratios are
/// the same).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Reading {
    pub now: f64,
    pub full: f64,
    pub rate: f64,
    pub status: Option<Charge>,
}

fn status(text: &str) -> Option<Charge> {
    match text.trim() {
        "Charging" => Some(Charge::Charging),
        "Discharging" => Some(Charge::Discharging),
        "Full" => Some(Charge::Full),
        "Not charging" => Some(Charge::NotCharging),
        _ => None,
    }
}

fn number(dir: &Path, name: &str) -> Option<f64> {
    std::fs::read_to_string(dir.join(name))
        .ok()?
        .trim()
        .parse()
        .ok()
}

fn read_one(dir: &Path) -> Option<Reading> {
    let (now, full, rate) = match number(dir, "energy_now") {
        Some(now) => (
            now,
            number(dir, "energy_full")?,
            number(dir, "power_now").unwrap_or(0.0),
        ),
        None => (
            number(dir, "charge_now")?,
            number(dir, "charge_full")?,
            number(dir, "current_now").unwrap_or(0.0),
        ),
    };
    let status = std::fs::read_to_string(dir.join("status"))
        .ok()
        .and_then(|s| status(&s));
    Some(Reading {
        now,
        full,
        rate: rate.abs(),
        status,
    })
}

/// The machine's own batteries' folders in sysfs.
fn batteries() -> Vec<std::path::PathBuf> {
    let Ok(entries) = std::fs::read_dir("/sys/class/power_supply") else {
        return Vec::new();
    };
    entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            std::fs::read_to_string(p.join("type")).is_ok_and(|t| t.trim() == "Battery")
                // Mice and headsets report batteries too; only the
                // machine's own power them.
                && std::fs::read_to_string(p.join("scope")).map_or(true, |s| s.trim() != "Device")
        })
        .collect()
}

/// The Linux kernel's account of the batteries.
pub struct Sysfs;

impl PowerSource for Sysfs {
    fn read(&self) -> Option<Battery> {
        let readings: Vec<Reading> = batteries().iter().filter_map(|p| read_one(p)).collect();
        combine(&readings)
    }

    fn health(&self) -> Option<Health> {
        batteries().iter().find_map(|dir| {
            let (full, design) = match number(dir, "energy_full") {
                Some(full) => (full, number(dir, "energy_full_design")?),
                None => (
                    number(dir, "charge_full")?,
                    number(dir, "charge_full_design")?,
                ),
            };
            (design > 0.0).then(|| Health {
                capacity: (full / design * 100.0).min(100.0),
                // 0 where it doesn't count them.
                cycles: number(dir, "cycle_count")
                    .filter(|n| *n > 0.0)
                    .map(|n| n as u32),
            })
        })
    }
}

/// Several batteries as one: summed charge, and the most telling status
/// (charging over discharging over the rest).
pub fn combine(readings: &[Reading]) -> Option<Battery> {
    let full: f64 = readings.iter().map(|r| r.full).sum();
    if full <= 0.0 {
        return None;
    }
    let now: f64 = readings.iter().map(|r| r.now).sum();
    let rate: f64 = readings.iter().map(|r| r.rate).sum();
    let has = |c: Charge| readings.iter().any(|r| r.status == Some(c));
    let charge = if has(Charge::Charging) {
        Charge::Charging
    } else if has(Charge::Discharging) {
        Charge::Discharging
    } else if has(Charge::NotCharging) {
        Charge::NotCharging
    } else {
        Charge::Full
    };
    let percent = ((now / full) * 100.0).round().clamp(0.0, 100.0) as u8;
    let hours = match charge {
        Charge::Discharging if rate > 0.0 => Some(now / rate),
        Charge::Charging if rate > 0.0 => Some((full - now).max(0.0) / rate),
        _ => None,
    };
    Some(Battery {
        percent,
        charge,
        // Above two days the draw is too small to mean anything.
        minutes: hours
            .filter(|h| *h < 48.0)
            .map(|h| (h * 60.0).round() as u32),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reading(now: f64, full: f64, rate: f64, status: Charge) -> Reading {
        Reading {
            now,
            full,
            rate,
            status: Some(status),
        }
    }

    #[test]
    fn nifls_battery() {
        let b = combine(&[reading(
            29_560_000.0,
            45_670_000.0,
            7_066_000.0,
            Charge::Discharging,
        )])
        .unwrap();
        assert_eq!(b.percent, 65);
        assert_eq!(b.minutes, Some(251));
        assert_eq!(b.describe(), "65% · 4 h 11 min left");
        assert!(!b.low());
    }

    #[test]
    fn charging_counts_time_to_full() {
        let b = combine(&[reading(40.0, 100.0, 30.0, Charge::Charging)]).unwrap();
        assert_eq!(b.minutes, Some(120));
        assert_eq!(b.describe(), "Charging 40% · 2 h 0 min to full");
    }

    #[test]
    fn two_batteries_are_one_and_charging_wins() {
        let b = combine(&[
            reading(10.0, 50.0, 0.0, Charge::Full),
            reading(40.0, 50.0, 5.0, Charge::Charging),
        ])
        .unwrap();
        assert_eq!(b.percent, 50);
        assert_eq!(b.charge, Charge::Charging);
    }

    #[test]
    fn low_only_when_discharging() {
        let low = combine(&[reading(8.0, 100.0, 10.0, Charge::Discharging)]).unwrap();
        assert!(low.low());
        let charging = combine(&[reading(8.0, 100.0, 10.0, Charge::Charging)]).unwrap();
        assert!(!charging.low());
    }

    #[test]
    fn no_battery_no_reading() {
        assert_eq!(combine(&[]), None);
    }
}
