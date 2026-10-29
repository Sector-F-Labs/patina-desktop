//! The battery as FreeBSD gives it: the ACPI battery's sysctls for the charge
//! and `acpiconf` for how worn it is. Each read runs a program, cheap enough
//! at the bar's pace.

use std::process::Command;

use super::{Battery, Charge, Health, PowerSource};

/// FreeBSD's ACPI battery.
pub struct Acpi;

impl PowerSource for Acpi {
    fn read(&self) -> Option<Battery> {
        let out = run(
            "sysctl",
            &[
                "-n",
                "hw.acpi.battery.life",
                "hw.acpi.battery.state",
                "hw.acpi.battery.time",
                "hw.acpi.acline",
            ],
        )?;
        battery(&out)
    }

    fn health(&self) -> Option<Health> {
        health(&run("acpiconf", &["-i", "0"])?)
    }
}

/// What the program printed, if it ran and succeeded.
fn run(program: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(program).args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// What `sysctl -n` prints for hw.acpi.battery.life, .state, .time and
/// hw.acpi.acline, one per line: the charge in percent, the state's bits
/// (1 discharging, 2 charging, 4 critical), the minutes left (-1 unknown)
/// and whether the charger is in. `None` without a battery (life is -1).
fn battery(text: &str) -> Option<Battery> {
    let values: Vec<i32> = text
        .split_whitespace()
        .map(|v| v.parse().ok())
        .collect::<Option<_>>()?;
    let [life, state, time, acline] = values[..] else {
        return None;
    };
    if !(0..=100).contains(&life) {
        return None;
    }
    let charge = if state & 1 != 0 {
        Charge::Discharging
    } else if state & 2 != 0 {
        Charge::Charging
    } else if life >= 100 {
        Charge::Full
    } else if acline != 0 {
        Charge::NotCharging
    } else {
        Charge::Discharging
    };
    Some(Battery {
        percent: life as u8,
        charge,
        // ACPI only estimates what's left; to full it doesn't say.
        minutes: u32::try_from(time)
            .ok()
            .filter(|m| charge == Charge::Discharging && *m < 48 * 60),
    })
}

/// What `acpiconf -i 0` prints: "Design capacity: 57000 mWh", "Last full
/// capacity: 48600 mWh" and "Cycle Count: 191".
fn health(text: &str) -> Option<Health> {
    let field = |name: &str| {
        text.lines().find_map(|line| {
            let value = line.strip_prefix(name)?.strip_prefix(':')?;
            value.split_whitespace().next()?.parse::<f64>().ok()
        })
    };
    let design = field("Design capacity").filter(|d| *d > 0.0)?;
    let full = field("Last full capacity")?;
    Some(Health {
        capacity: (full / design * 100.0).min(100.0),
        cycles: field("Cycle Count").filter(|n| *n > 0.0).map(|n| n as u32),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lokis_battery_charging() {
        let b = battery("62\n2\n-1\n1\n").unwrap();
        assert_eq!(
            (b.percent, b.charge, b.minutes),
            (62, Charge::Charging, None)
        );
        assert_eq!(b.describe(), "Charging 62%");
    }

    #[test]
    fn discharging_has_minutes() {
        let b = battery("40\n1\n95\n0\n").unwrap();
        assert_eq!(b.describe(), "40% · 1 h 35 min left");
    }

    #[test]
    fn plugged_in_full_or_holding() {
        assert_eq!(battery("100\n0\n-1\n1\n").unwrap().charge, Charge::Full);
        assert_eq!(
            battery("80\n0\n-1\n1\n").unwrap().charge,
            Charge::NotCharging
        );
    }

    #[test]
    fn without_a_battery() {
        assert_eq!(battery("-1\n0\n-1\n1\n"), None);
        assert_eq!(battery(""), None);
    }

    #[test]
    fn lokis_acpiconf_health() {
        let h = health(
            "Design capacity:\t57000 mWh\nLast full capacity:\t48600 mWh\nCycle Count:\t191\n",
        )
        .unwrap();
        assert_eq!(
            h.describe(),
            "Holds 85% of its original capacity · 191 charge cycles"
        );
    }
}
