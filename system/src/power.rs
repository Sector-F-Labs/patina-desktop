//! The battery: the charge, whether it's charging, and the time left at the
//! current draw (ADR-0024); laptops with two batteries count as one. Also how
//! worn it is (`health`); its history is `battery_history`.
//!
//! Where that's read from is a `PowerSource`: Linux's `/sys/class/power_supply`
//! (`sysfs`), or FreeBSD's ACPI sysctls and `acpiconf` (`acpi`, ADR-0095). The
//! one for the machine is picked when asked, so both build and test anywhere.

mod acpi;
mod sysfs;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Charge {
    Charging,
    Discharging,
    Full,
    /// Plugged in but holding (a charge threshold, or warm).
    NotCharging,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Battery {
    /// 0 to 100.
    pub percent: u8,
    pub charge: Charge,
    /// Minutes until empty (discharging) or full (charging), when the
    /// battery reports its power draw.
    pub minutes: Option<u32>,
}

impl Battery {
    /// Low enough to warn about: 10% or under, and not charging.
    pub fn low(&self) -> bool {
        self.percent <= 10 && self.charge == Charge::Discharging
    }

    /// "65% · 4 h 10 min left", for a tooltip.
    pub fn describe(&self) -> String {
        let time = self.minutes.map(|m| {
            if m >= 60 {
                format!("{} h {} min", m / 60, m % 60)
            } else {
                format!("{m} min")
            }
        });
        match (self.charge, time) {
            (Charge::Discharging, Some(t)) => format!("{}% · {t} left", self.percent),
            (Charge::Charging, Some(t)) => format!("Charging {}% · {t} to full", self.percent),
            (Charge::Charging, None) => format!("Charging {}%", self.percent),
            (Charge::Full, _) => "Fully charged".into(),
            (Charge::NotCharging, _) => format!("{}% · plugged in, not charging", self.percent),
            (Charge::Discharging, None) => format!("{}%", self.percent),
        }
    }
}

/// How worn the battery is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Health {
    /// What it holds when full, as a percentage of what it held new.
    pub capacity: f64,
    /// Charge cycles, where the battery counts them.
    pub cycles: Option<u32>,
}

impl Health {
    /// "Holds 80% of its original capacity · 126 charge cycles".
    pub fn describe(&self) -> String {
        let held = format!("Holds {:.0}% of its original capacity", self.capacity);
        match self.cycles {
            Some(1) => format!("{held} · 1 charge cycle"),
            Some(n) => format!("{held} · {n} charge cycles"),
            None => held,
        }
    }
}

/// Where a machine keeps what its battery says.
trait PowerSource {
    /// The batteries combined, or `None` on a machine without one.
    fn read(&self) -> Option<Battery>;
    /// The first battery's health, where it says what it held new.
    fn health(&self) -> Option<Health>;
}

/// sysfs where the machine has it, else ACPI as FreeBSD gives it.
fn source() -> &'static dyn PowerSource {
    if std::path::Path::new("/sys/class/power_supply").is_dir() {
        &sysfs::Sysfs
    } else {
        &acpi::Acpi
    }
}

/// The batteries combined, or `None` on a machine without one.
pub fn read() -> Option<Battery> {
    source().read()
}

/// The first battery's health, where it says what it held new.
pub fn health() -> Option<Health> {
    source().health()
}
