//! The battery's history, as UPower keeps it: the charge and the power
//! draw over time, recorded as they change, for Settings' graphs. Linux
//! only in practice; `None` without UPower or a battery.

use zbus::blocking::{Connection, Proxy};

use crate::power::Charge;

const UPOWER: &str = "org.freedesktop.UPower";
const DEVICE: &str = "org.freedesktop.UPower.Device";
/// UPower's type for a battery.
const BATTERY: u32 = 2;

/// One recorded value: a percentage for the charge, watts for the draw.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    /// Seconds since the epoch.
    pub time: u64,
    pub value: f64,
    /// `None` where UPower didn't know.
    pub state: Option<Charge>,
}

impl Sample {
    /// Plugged in: charging, full, or holding.
    pub fn plugged(&self) -> bool {
        matches!(
            self.state,
            Some(Charge::Charging | Charge::Full | Charge::NotCharging)
        )
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct History {
    /// Oldest first.
    pub charge: Vec<Sample>,
    pub rate: Vec<Sample>,
}

/// UPower's states: 1 charging, 2 discharging, 3 empty, 4 full, 5 pending
/// charge, 6 pending discharge.
fn state(n: u32) -> Option<Charge> {
    match n {
        1 => Some(Charge::Charging),
        2 | 3 | 6 => Some(Charge::Discharging),
        4 => Some(Charge::Full),
        5 => Some(Charge::NotCharging),
        _ => None,
    }
}

/// UPower's answer, newest first, as samples oldest first; without the
/// unknown zeroes it records at boot.
pub fn samples(points: &[(u32, f64, u32)]) -> Vec<Sample> {
    let mut samples: Vec<Sample> = points
        .iter()
        .filter(|(_, value, s)| !(*s == 0 && *value == 0.0))
        .map(|&(time, value, s)| Sample {
            time: u64::from(time),
            value,
            state: state(s),
        })
        .collect();
    samples.sort_by_key(|s| s.time);
    samples
}

/// The machine's own battery, the first one powering it.
fn battery(bus: &Connection) -> Result<Option<Proxy<'static>>, String> {
    let err = |e: zbus::Error| e.to_string();
    let upower = Proxy::new(bus, UPOWER, "/org/freedesktop/UPower", UPOWER).map_err(err)?;
    let devices: Vec<zbus::zvariant::OwnedObjectPath> =
        match upower.call("EnumerateDevices", &()) {
            Ok(d) => d,
            // Not installed.
            Err(zbus::Error::MethodError(..)) => return Ok(None),
            Err(e) => return Err(e.to_string()),
        };
    for path in devices {
        let device = Proxy::new(bus, UPOWER, path, DEVICE).map_err(err)?;
        if device.get_property::<u32>("Type").ok() == Some(BATTERY)
            && device.get_property::<bool>("PowerSupply").unwrap_or(false)
        {
            return Ok(Some(device));
        }
    }
    Ok(None)
}

/// The last `seconds` of history, in at most `points` points each.
pub fn read(seconds: u32, points: u32) -> Result<Option<History>, String> {
    let bus = Connection::system().map_err(|e| format!("no system bus: {e}"))?;
    let Some(device) = battery(&bus)? else {
        return Ok(None);
    };
    let get = |kind: &str| -> Result<Vec<Sample>, String> {
        let points: Vec<(u32, f64, u32)> = device
            .call("GetHistory", &(kind, seconds, points))
            .map_err(|e| e.to_string())?;
        Ok(samples(&points))
    };
    Ok(Some(History {
        charge: get("charge")?,
        rate: get("rate")?,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// nifl's, trimmed: newest first, with the boot's unknown zero.
    #[test]
    fn upowers_history_oldest_first_without_the_boot_zero() {
        let s = samples(&[
            (1792739825, 82.0, 0),
            (1792739564, 83.5, 2),
            (1792694759, 0.0, 0),
            (1792693369, 50.5, 1),
        ]);
        assert_eq!(
            s.iter().map(|s| s.time).collect::<Vec<_>>(),
            [1792693369, 1792739564, 1792739825]
        );
        assert!(s[0].plugged());
        assert_eq!(s[1].state, Some(Charge::Discharging));
        assert_eq!(s[2].state, None);
    }
}
